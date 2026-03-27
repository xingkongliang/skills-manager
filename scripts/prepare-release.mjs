#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const args = process.argv.slice(2);

const releaseArg = args.find((arg) => !arg.startsWith('--'));
const dryRun = args.includes('--dry-run');

if (!releaseArg) {
  console.error('Usage: npm run release:prepare -- <patch|minor|major|x.y.z> [--dry-run]');
  process.exit(1);
}

const dateStr = new Date().toISOString().slice(0, 10);

const packagePath = path.join(root, 'package.json');
const tauriConfPath = path.join(root, 'src-tauri', 'tauri.conf.json');
const enI18nPath = path.join(root, 'src', 'i18n', 'en.json');
const zhI18nPath = path.join(root, 'src', 'i18n', 'zh.json');
const changelogPath = path.join(root, 'CHANGELOG.md');
const changelogZhPath = path.join(root, 'CHANGELOG-zh.md');

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function writeJson(filePath, value) {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`);
}

function parseSemver(version) {
  const m = version.match(/^(\d+)\.(\d+)\.(\d+)$/);
  if (!m) return null;
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]) };
}

function bumpVersion(current, releaseType) {
  const parsed = parseSemver(current);
  if (!parsed) {
    throw new Error(`Current package version is not SemVer: ${current}`);
  }

  if (releaseType === 'patch') {
    return `${parsed.major}.${parsed.minor}.${parsed.patch + 1}`;
  }
  if (releaseType === 'minor') {
    return `${parsed.major}.${parsed.minor + 1}.0`;
  }
  if (releaseType === 'major') {
    return `${parsed.major + 1}.0.0`;
  }

  if (parseSemver(releaseType)) {
    return releaseType;
  }

  throw new Error(`Invalid release type/version: ${releaseType}`);
}

function updateSettingsVersion(i18nObj, nextVersion, fileLabel) {
  if (!i18nObj.settings || typeof i18nObj.settings.version !== 'string') {
    throw new Error(`Missing settings.version in ${fileLabel}`);
  }
  i18nObj.settings.version = i18nObj.settings.version.replace(/\d+\.\d+\.\d+/, nextVersion);
}

function ensureChangelogEntry(changelog, nextVersion, { zh = false } = {}) {
  const heading = `## [${nextVersion}] - ${dateStr}`;
  if (changelog.includes(heading) || changelog.includes(`## [${nextVersion}] -`)) {
    return changelog;
  }

  const sections = zh
    ? ['### 新增', '- ', '', '### 变更', '- ', '', '### 修复', '- ', '', '### 移除', '- ']
    : ['### Added', '- ', '', '### Changed', '- ', '', '### Fixed', '- ', '', '### Removed', '- '];

  const entry = [heading, '', ...sections, ''].join('\n');

  const firstReleaseHeading = changelog.search(/^## \[/m);
  if (firstReleaseHeading === -1) {
    return `${changelog.trimEnd()}\n\n${entry}\n`;
  }

  return `${changelog.slice(0, firstReleaseHeading)}${entry}${changelog.slice(firstReleaseHeading)}`;
}

function main() {
  const pkg = readJson(packagePath);
  const tauriConf = readJson(tauriConfPath);
  const en = readJson(enI18nPath);
  const zh = readJson(zhI18nPath);
  const changelog = fs.readFileSync(changelogPath, 'utf8');
  const changelogZh = fs.readFileSync(changelogZhPath, 'utf8');

  const currentVersion = pkg.version;
  const nextVersion = bumpVersion(currentVersion, releaseArg);

  pkg.version = nextVersion;
  tauriConf.version = nextVersion;
  updateSettingsVersion(en, nextVersion, 'src/i18n/en.json');
  updateSettingsVersion(zh, nextVersion, 'src/i18n/zh.json');
  const nextChangelog = ensureChangelogEntry(changelog, nextVersion);
  const nextChangelogZh = ensureChangelogEntry(changelogZh, nextVersion, { zh: true });

  if (dryRun) {
    console.log(`[dry-run] ${currentVersion} -> ${nextVersion}`);
    return;
  }

  writeJson(packagePath, pkg);
  writeJson(tauriConfPath, tauriConf);
  writeJson(enI18nPath, en);
  writeJson(zhI18nPath, zh);
  fs.writeFileSync(changelogPath, nextChangelog);
  fs.writeFileSync(changelogZhPath, nextChangelogZh);

  console.log(`Prepared release ${nextVersion}`);
  console.log('Updated:');
  console.log('- CHANGELOG.md');
  console.log('- CHANGELOG-zh.md');
  console.log('- package.json');
  console.log('- src-tauri/tauri.conf.json');
  console.log('- src/i18n/en.json');
  console.log('- src/i18n/zh.json');
}

main();
