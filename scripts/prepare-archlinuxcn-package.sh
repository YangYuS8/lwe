#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
Usage: scripts/prepare-archlinuxcn-package.sh <stable|git> <pkgver> <output-root>

Writes archlinuxcn/repo-ready package metadata under:
  <output-root>/archlinuxcn/<package-name>/

Environment overrides:
  LWE_ARCHLINUXCN_GITHUB_REPOSITORY   GitHub source repo, default YangYuS8/lwe
  LWE_ARCHLINUXCN_PROJECT_URL         Project URL written into PKGBUILD
  LWE_ARCHLINUXCN_GIT_URL             Git source URL for lwe-git
  LWE_ARCHLINUXCN_SOURCE_COMMIT       Optional exact commit for lwe-git source
  LWE_ARCHLINUXCN_RELEASE_ASSET_URL   Optional stable .deb release asset URL
  LWE_ARCHLINUXCN_MAINTAINER_GITHUB   lilac maintainer GitHub login
  LWE_ARCHLINUXCN_MAINTAINER_EMAIL    Optional lilac maintainer email
USAGE
}

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
	usage
	exit 0
fi

if [ "$#" -ne 3 ]; then
	usage
	exit 1
fi

channel="$1"
pkgver="$2"
output_root="$3"

github_repository="${LWE_ARCHLINUXCN_GITHUB_REPOSITORY:-YangYuS8/lwe}"
project_url="${LWE_ARCHLINUXCN_PROJECT_URL:-https://github.com/${github_repository}}"
git_url="${LWE_ARCHLINUXCN_GIT_URL:-${project_url}.git}"
pkgrel="${LWE_ARCHLINUXCN_PKGREL:-1}"
maintainer_github="${LWE_ARCHLINUXCN_MAINTAINER_GITHUB:-YangYuS8}"
maintainer_email="${LWE_ARCHLINUXCN_MAINTAINER_EMAIL:-}"

write_maintainers() {
	local lilac_file="$1"

	{
		printf 'maintainers:\n'
		printf '  - github: %s\n' "${maintainer_github}"
		if [ -n "${maintainer_email}" ]; then
			printf '    email: %s\n' "${maintainer_email}"
		fi
	} >"${lilac_file}"
}

case "${channel}" in
	stable)
		pkgname="lwe"
		source_pkgdir="packaging/aur/lwe"
		release_asset_url="${LWE_ARCHLINUXCN_RELEASE_ASSET_URL:-${project_url}/releases/download/v${pkgver}/lwe_${pkgver}_amd64.deb}"
		;;
	git)
		pkgname="lwe-git"
		source_pkgdir="packaging/aur/lwe-git"
		source_commit="${LWE_ARCHLINUXCN_SOURCE_COMMIT:-}"
		if [ -n "${source_commit}" ]; then
			git_source="git+${git_url}#commit=${source_commit}"
		else
			git_source="git+${git_url}"
		fi
		;;
	*)
		echo "Unknown ArchLinuxCN channel: ${channel}" >&2
		usage
		exit 1
		;;
esac

package_dir="${output_root}/archlinuxcn/${pkgname}"
rm -rf "${package_dir}"
mkdir -p "${package_dir}"

cp "${source_pkgdir}/PKGBUILD" "${package_dir}/PKGBUILD"
sed -i "s/^pkgver=.*/pkgver=${pkgver}/" "${package_dir}/PKGBUILD"
sed -i "s/^pkgrel=.*/pkgrel=${pkgrel}/" "${package_dir}/PKGBUILD"
sed -i "s|^url=.*|url=\"${project_url}\"|" "${package_dir}/PKGBUILD"

case "${channel}" in
	stable)
		sed -i "s|^source=.*|source=(\"lwe_${pkgver}_amd64.deb::${release_asset_url}\")|" "${package_dir}/PKGBUILD"
		write_maintainers "${package_dir}/lilac.yaml"
		cat >>"${package_dir}/lilac.yaml" <<EOF

build_prefix: extra-x86_64
pre_build_script: update_pkgver_and_pkgrel(_G.newver)
post_build: git_pkgbuild_commit

update_on:
  - source: github
    github: ${github_repository}
    use_latest_release: true
    prefix: v
EOF
		;;
	git)
		sed -i "s|^source=.*|source=(\"${git_source}\")|" "${package_dir}/PKGBUILD"
		write_maintainers "${package_dir}/lilac.yaml"
		if [ -n "${source_commit:-}" ]; then
			cat >>"${package_dir}/lilac.yaml" <<'EOF'

build_prefix: extra-x86_64

update_on:
  - source: manual
    manual: 1
EOF
		else
			cat >>"${package_dir}/lilac.yaml" <<EOF

build_prefix: extra-x86_64
pre_build: vcs_update
post_build: git_pkgbuild_commit

update_on:
  - source: github
    github: ${github_repository}
EOF
		fi
		;;
esac

printf 'Prepared ArchLinuxCN metadata in %s\n' "${package_dir}"
