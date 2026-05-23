#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
Usage: scripts/generate-release-notes.sh <tag-or-title> [output-file]

Generates GitHub Release notes from commits since the previous reachable stable
tag. Prerelease tags are intentionally ignored so stable releases summarize the
full stable-to-stable delta. If no previous stable tag exists, all reachable
commits are included.
USAGE
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
	usage
	exit 0
fi

release_name="${1:-${GITHUB_REF_NAME:-HEAD}}"
output_file="${2:-release-notes.md}"

current_ref="${RELEASE_NOTES_REF:-HEAD}"
previous_tag="$(
	git tag --merged "${current_ref}^" --sort=-v:refname 2>/dev/null |
		awk '/^v[0-9]+[.][0-9]+[.][0-9]+$/ { print; exit }'
)"

if [ -n "${previous_tag}" ]; then
	revision_range="${previous_tag}..${current_ref}"
	compare_base="${previous_tag}"
else
	revision_range="${current_ref}"
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
