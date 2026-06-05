#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
Usage: scripts/open-archlinuxcn-pr.sh <prepared-root> <package-name> <branch-name> <commit-message> <pr-title> <pr-body-file>

Copies <prepared-root>/archlinuxcn/<package-name> into a branch on an
archlinuxcn/repo fork and opens or updates a pull request.

Required environment for PR mode:
  ARCHLINUXCN_GITHUB_TOKEN   GitHub token with access to the fork and PR API
  ARCHLINUXCN_REPO_FORK      Fork full name, for example YangYuS8/repo

Optional environment:
  ARCHLINUXCN_REPO_UPSTREAM     Upstream repo, default archlinuxcn/repo
  ARCHLINUXCN_REPO_BASE_BRANCH  Base branch, default master
USAGE
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
	usage
	exit 0
fi

if [ "$#" -ne 6 ]; then
	usage
	exit 1
fi

prepared_root="$1"
package_name="$2"
branch_name="$3"
commit_message="$4"
pr_title="$5"
pr_body_file="$6"

if [ -z "${ARCHLINUXCN_GITHUB_TOKEN:-}" ] || [ -z "${ARCHLINUXCN_REPO_FORK:-}" ]; then
	echo "ARCHLINUXCN_GITHUB_TOKEN or ARCHLINUXCN_REPO_FORK is not configured; skipping ArchLinuxCN PR creation."
	exit 0
fi

if [ ! -d "${prepared_root}/archlinuxcn/${package_name}" ]; then
	echo "Prepared package directory not found: ${prepared_root}/archlinuxcn/${package_name}" >&2
	exit 1
fi

if [ ! -f "${pr_body_file}" ]; then
	echo "PR body file not found: ${pr_body_file}" >&2
	exit 1
fi

prepared_package_dir="$(cd "${prepared_root}/archlinuxcn/${package_name}" && pwd)"
pr_body_abs="$(cd "$(dirname "${pr_body_file}")" && pwd)/$(basename "${pr_body_file}")"
upstream="${ARCHLINUXCN_REPO_UPSTREAM:-archlinuxcn/repo}"
base_branch="${ARCHLINUXCN_REPO_BASE_BRANCH:-master}"
workdir="archlinuxcn-repo"
fork_owner="${ARCHLINUXCN_REPO_FORK%%/*}"
head_ref="${fork_owner}:${branch_name}"

rm -rf "${workdir}"
git clone "https://github.com/${upstream}.git" "${workdir}"

cd "${workdir}"
git checkout -B "${branch_name}" "origin/${base_branch}"

package_path="archlinuxcn/${package_name}"
rm -rf "${package_path}"
mkdir -p "${package_path}"
cp -R "${prepared_package_dir}/." "${package_path}/"

git add "${package_path}"
if git diff --cached --quiet; then
	echo "No ArchLinuxCN metadata changes to publish for ${package_name}"
	exit 0
fi

git config user.name "${ARCHLINUXCN_GIT_USER_NAME:-cnb-bot}"
git config user.email "${ARCHLINUXCN_GIT_USER_EMAIL:-cnb-bot@users.noreply.cnb.cool}"
git commit -m "${commit_message}"

auth_header="$(printf 'x-access-token:%s' "${ARCHLINUXCN_GITHUB_TOKEN}" | base64 | tr -d '\n')"
git -c "http.https://github.com/.extraheader=AUTHORIZATION: basic ${auth_header}" \
	push "https://github.com/${ARCHLINUXCN_REPO_FORK}.git" "HEAD:${branch_name}" --force

node ../scripts/open-archlinuxcn-pr.mjs \
	"${upstream}" \
	"${head_ref}" \
	"${base_branch}" \
	"${pr_title}" \
	"${pr_body_abs}"
