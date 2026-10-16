// Run only in a disposable container with no host home/config mounts.
// This checks provider CLI contracts, not the application's binding persistence.
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';

assert(existsSync('/.dockerenv'), 'Use the disposable Docker command in docs/v1.38.0-port.md');
const root = mkdtempSync(join(tmpdir(), 'qskills-native-'));
const market = 'qskills-smoke';
const plugin = 'smoke';
const selector = `${plugin}@${market}`;
const put = (path, value) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, typeof value === 'string' ? value : JSON.stringify(value));
};
const run = (tool, ...args) => {
  const result = execFileSync(tool, args, { cwd: root, encoding: 'utf8', timeout: 60000 });
  console.log(`$ ${tool} ${args.join(' ')}\n${result.trim()}`);
  return result;
};
const repo = join(root, 'marketplace');
const setVersion = version => {
  const manifest = { name: plugin, version, description: 'Harmless native lifecycle fixture' };
  for (const dir of ['.claude-plugin', '.codex-plugin']) {
    put(join(repo, 'plugins', plugin, dir, 'plugin.json'), manifest);
  }
  put(join(repo, 'plugins', plugin, 'skills', 'smoke', 'SKILL.md'),
    `---\nname: smoke\ndescription: Native lifecycle fixture\n---\nReport fixture ${version}.\n`);
  put(join(repo, '.claude-plugin', 'marketplace.json'), {
    name: market, owner: { name: 'QSkillsMan test' },
    plugins: [{ name: plugin, source: './plugins/smoke', version }],
  });
  put(join(repo, '.agents', 'plugins', 'marketplace.json'), {
    name: market, plugins: [{ name: plugin, source: { source: 'local', path: './plugins/smoke' } }],
  });
};
setVersion('1.0.0');
run('claude', '--version');
run('codex', '--version');
run('claude', 'plugin', 'marketplace', 'add', '--scope', 'user', repo);
run('claude', 'plugin', 'install', '--scope', 'user', selector);
const claudeVersion = () => JSON.parse(run('claude', 'plugin', 'list', '--json'))
  .find(entry => entry.id === selector)?.version;
assert.equal(claudeVersion(), '1.0.0');
setVersion('1.1.0');
run('claude', 'plugin', 'marketplace', 'update', market);
run('claude', 'plugin', 'update', '--scope', 'user', selector);
assert.equal(claudeVersion(), '1.1.0');
run('claude', 'plugin', 'uninstall', '--scope', 'user', selector);
assert(!run('claude', 'plugin', 'list', '--json').includes(selector));
run('claude', 'plugin', 'marketplace', 'remove', '--scope', 'user', market);

run('codex', 'plugin', 'marketplace', 'add', repo, '--json');
const codexInstalled = () => JSON.parse(run('codex', 'plugin', 'list', '--json')).installed
  .find(entry => entry.pluginId === selector);
const installed = JSON.parse(run('codex', 'plugin', 'add', selector, '--json'));
assert.equal(codexInstalled()?.version, '1.1.0');
assert(existsSync(installed.installedPath));
run('codex', 'plugin', 'remove', selector, '--json');
assert.equal(codexInstalled(), undefined);
assert(!existsSync(installed.installedPath));
// Codex has no native update command: verify explicit remove/add replacement.
setVersion('1.2.0');
run('codex', 'plugin', 'add', selector, '--json');
assert.equal(codexInstalled()?.version, '1.2.0');
run('codex', 'plugin', 'remove', selector, '--json');
assert.equal(codexInstalled(), undefined);
run('codex', 'plugin', 'marketplace', 'remove', market, '--json');
assert(!run('codex', 'plugin', 'list', '--json').includes(market));
console.log('PASS: Claude native upgrade; Codex generated marketplace install/replace/remove');
