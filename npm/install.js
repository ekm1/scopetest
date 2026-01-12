#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const PLATFORMS = {
  'darwin-arm64': '@scopetest/darwin-arm64',
  'darwin-x64': '@scopetest/darwin-x64',
  'linux-x64': '@scopetest/linux-x64-gnu',
  'linux-arm64': '@scopetest/linux-arm64-gnu',
  'win32-x64': '@scopetest/win32-x64-msvc',
};

function getPlatformPackage() {
  const platform = process.platform;
  const arch = process.arch;
  const key = `${platform}-${arch}`;
  return PLATFORMS[key];
}

function main() {
  const platformPkg = getPlatformPackage();
  
  if (!platformPkg) {
    console.error(`Unsupported platform: ${process.platform}-${process.arch}`);
    console.error('Supported platforms: darwin-arm64, darwin-x64, linux-x64, linux-arm64, win32-x64');
    process.exit(1);
  }

  try {
    const binaryPath = require.resolve(`${platformPkg}/scopetest`);
    const binDir = path.join(__dirname, '..', 'bin');
    const targetPath = path.join(binDir, 'scopetest');

    if (!fs.existsSync(binDir)) {
      fs.mkdirSync(binDir, { recursive: true });
    }

    // Copy binary to bin directory
    fs.copyFileSync(binaryPath, targetPath);
    fs.chmodSync(targetPath, 0o755);
    
    console.log(`scopetest installed successfully for ${process.platform}-${process.arch}`);
  } catch (err) {
    console.error(`Failed to install scopetest: ${err.message}`);
    console.error('You may need to build from source: cargo build --release');
    process.exit(1);
  }
}

main();
