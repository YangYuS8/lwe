#!/usr/bin/env node
import fs from 'node:fs';
import process from 'node:process';

const [upstream, headRef, baseBranch, title, bodyFile] = process.argv.slice(2);
const token = process.env.ARCHLINUXCN_GITHUB_TOKEN;

if (!upstream || !headRef || !baseBranch || !title || !bodyFile) {
  console.error('Usage: open-archlinuxcn-pr.mjs <upstream> <head-ref> <base-branch> <title> <body-file>');
  process.exit(1);
}

if (!token) {
  console.error('ARCHLINUXCN_GITHUB_TOKEN is required');
  process.exit(1);
}

const body = fs.readFileSync(bodyFile, 'utf8');

async function githubRequest(method, path, payload) {
  const response = await fetch(`https://api.github.com${path}`, {
    method,
    headers: {
      Accept: 'application/vnd.github+json',
      Authorization: `Bearer ${token}`,
      'User-Agent': 'lwe-cnb-archlinuxcn-publisher',
      'X-GitHub-Api-Version': '2022-11-28',
      ...(payload ? { 'Content-Type': 'application/json' } : {})
    },
    body: payload ? JSON.stringify(payload) : undefined
  });

  const text = await response.text();
  const data = text ? JSON.parse(text) : {};

  if (!response.ok) {
    const message = data.message ? `: ${data.message}` : '';
    throw new Error(`GitHub API ${method} ${path} failed with ${response.status}${message}`);
  }

  return data;
}

function manualPullRequestUrl() {
  const [forkOwner] = headRef.split(':')[0].split('/');
  const headBranch = headRef.split(':')[1];

  if (!forkOwner || !headBranch) {
    return `https://github.com/${upstream}/pulls`;
  }

  const query = new URLSearchParams({
    quick_pull: '1',
    title,
    body,
    labels: '',
    template: '',
    base: `${upstream.split('/')[0]}:${baseBranch}`,
    head: `${forkOwner}:${headBranch}`
  });

  return `https://github.com/${upstream}/compare/${baseBranch}...${forkOwner}:${headBranch}?${query}`;
}

try {
  const query = new URLSearchParams({
    state: 'open',
    head: headRef,
    base: baseBranch
  });
  const existingPulls = await githubRequest('GET', `/repos/${upstream}/pulls?${query}`);

  if (existingPulls.length > 0) {
    const pull = existingPulls[0];
    const updated = await githubRequest('PATCH', `/repos/${upstream}/pulls/${pull.number}`, {
      title,
      body
    });
    console.log(`Updated ArchLinuxCN pull request: ${updated.html_url}`);
  } else {
    const created = await githubRequest('POST', `/repos/${upstream}/pulls`, {
      title,
      head: headRef,
      base: baseBranch,
      body,
      maintainer_can_modify: true
    });
    console.log(`Created ArchLinuxCN pull request: ${created.html_url}`);
  }
} catch (error) {
  if (error.message.includes('failed with 403')) {
    console.warn(
      `ArchLinuxCN branch was pushed, but the token cannot open or update the upstream pull request: ${error.message}`
    );
    console.warn(`Open the pull request manually: ${manualPullRequestUrl()}`);
  } else {
    throw error;
  }
}
