import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';

function canRun(command, args = ['--version']) {
  const result = spawnSync(command, args, { stdio: 'ignore' });
  return result.status === 0;
}

function resolveCargo() {
  if (process.env.CARGO && existsSync(process.env.CARGO)) {
    return process.env.CARGO;
  }

  if (canRun('cargo')) {
    return 'cargo';
  }

  const rustupCheck = spawnSync('rustup', ['which', 'rustc'], { encoding: 'utf8' });
  if (rustupCheck.status === 0) {
    const rustcPath = rustupCheck.stdout.trim();
    if (rustcPath) {
      const cargoPath = join(dirname(rustcPath), 'cargo');
      if (existsSync(cargoPath)) {
        return cargoPath;
      }
    }
  }

  console.error('cargo not found. Install Rust or ensure cargo/rustup is on PATH.');
  process.exit(127);
}

const mode = process.argv[2];
const extraArgs = process.argv.slice(3);
const cargo = resolveCargo();

const baseArgs = ['--manifest-path', 'src-tauri/Cargo.toml', '--bin', 'skills-manager-cli'];
const cargoArgs =
  mode === 'cli'
    ? ['run', '--quiet', ...baseArgs, '--', ...extraArgs]
    : mode === 'build'
      ? ['build', ...baseArgs]
      : null;

if (!cargoArgs) {
  console.error(`unknown mode: ${mode}`);
  process.exit(2);
}

const result = spawnSync(cargo, cargoArgs, {
  stdio: 'inherit',
  env: {
    ...process.env,
    PATH: `${dirname(cargo)}:${process.env.PATH ?? ''}`,
  },
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status ?? 1);
