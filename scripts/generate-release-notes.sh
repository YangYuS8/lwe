#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
Usage: scripts/generate-release-notes.sh <tag-or-title> [output-file]

Generates release notes from commits since the previous reachable stable tag.
Prerelease tags are intentionally ignored so stable releases summarize the full
stable-to-stable delta. The script fetches release tags first because some CI
checkouts do not include tags or complete history.
USAGE
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
	usage
	exit 0
fi

release_name="${1:-${GITHUB_REF_NAME:-HEAD}}"
output_file="${2:-release-notes.md}"

current_ref="${RELEASE_NOTES_REF:-HEAD}"
fallback_commits="${RELEASE_NOTES_FALLBACK_COMMITS:-50}"
fallback_note=""

fetch_release_history() {
	local remote="${RELEASE_NOTES_REMOTE:-origin}"

	if ! git remote get-url "${remote}" >/dev/null 2>&1; then
		return 0
	fi

	if [ "$(git rev-parse --is-shallow-repository 2>/dev/null || printf false)" = "true" ]; then
		git fetch --force --tags --prune --unshallow "${remote}" 2>/dev/null ||
			git fetch --force --tags --prune --deepen=1000 "${remote}" 2>/dev/null ||
			git fetch --force --tags --prune "${remote}" 2>/dev/null ||
			true
	else
		git fetch --force --tags --prune "${remote}" 2>/dev/null || true
	fi
}

stable_tag_search_ref="${current_ref}^"
if ! git rev-parse --verify "${stable_tag_search_ref}^{commit}" >/dev/null 2>&1; then
	stable_tag_search_ref="${current_ref}"
fi

fetch_release_history
previous_tag="$(
	git tag --merged "${stable_tag_search_ref}" --sort=-v:refname 2>/dev/null |
		awk '/^v[0-9]+[.][0-9]+[.][0-9]+$/ { print; exit }'
)"

if [ -n "${previous_tag}" ]; then
	revision_range="${previous_tag}..${current_ref}"
	compare_base="${previous_tag}"
else
	commit_count="$(git rev-list --count "${current_ref}")"
	if [ "${commit_count}" -gt "${fallback_commits}" ]; then
		revision_range="${current_ref}~${fallback_commits}..${current_ref}"
		fallback_note="No previous stable tag was visible in this checkout, so this note is limited to the latest ${fallback_commits} commits."
	else
		revision_range="${current_ref}"
		fallback_note="No previous stable tag was visible in this checkout."
	fi
	compare_base=""
fi

tmp_file="$(mktemp)"
remaining_file="$(mktemp)"
next_remaining_file="$(mktemp)"
trap 'rm -f "${tmp_file}" "${remaining_file}" "${next_remaining_file}"' EXIT

git log --no-merges --format='%h%x09%s%x09%an' "${revision_range}" >"${tmp_file}"
cp "${tmp_file}" "${remaining_file}"

write_category() {
	local title="$1"
	local pattern="$2"
	local matches
	matches="$(awk -F '\t' -v pattern="${pattern}" 'tolower($2) ~ pattern { print }' "${remaining_file}")"

	if [ -z "${matches}" ]; then
		return 0
	fi

	printf '## %s\n\n' "${title}"
	printf '%s\n' "${matches}" | while IFS=$'\t' read -r short_sha subject author; do
		printf -- '- %s (`%s`, %s)\n' "${subject}" "${short_sha}" "${author}"
	done
	printf '\n'

	awk -F '\t' -v pattern="${pattern}" 'tolower($2) !~ pattern { print }' "${remaining_file}" >"${next_remaining_file}"
	mv "${next_remaining_file}" "${remaining_file}"
}

{
	printf '# %s\n\n' "${release_name}"
	printf 'Automated release notes generated from commits'
	if [ -n "${compare_base}" ]; then
		printf ' since `%s`' "${compare_base}"
	fi
	printf '.\n\n'
	if [ -n "${fallback_note}" ]; then
		printf '%s\n\n' "${fallback_note}"
	fi

	if [ ! -s "${tmp_file}" ]; then
		printf 'No user-visible commit changes were found for this release.\n\n'
	else
		write_category "Features" '^(feat|feature)([(].+[)])?!?:|^add |^implement '
		write_category "Bug Fixes" '^(fix|bugfix)([(].+[)])?!?:|^repair |^resolve '
		write_category "Documentation" '^docs([(].+[)])?!?:|documentation|readme'
		write_category "Packaging and CI" '^(ci|build|release|packaging)([(].+[)])?!?:|workflow|aur|appimage|deb|rpm'
		write_category "Maintenance" '^(chore|refactor|test|style|perf)([(].+[)])?!?:|cleanup|simplify'

		other_matches="$(cat "${remaining_file}")"
		if [ -n "${other_matches}" ]; then
			printf '## Other Changes\n\n'
			printf '%s\n' "${other_matches}" | while IFS=$'\t' read -r short_sha subject author; do
				printf -- '- %s (`%s`, %s)\n' "${subject}" "${short_sha}" "${author}"
			done
			printf '\n'
		fi
	fi

	printf '## Install\n\n'
	printf 'Download the package for your distribution from the assets below. For AppImage builds, make the file executable before running it.\n'
} >"${output_file}"

printf 'Wrote release notes to %s\n' "${output_file}"
