import { readFileSync } from "node:fs";

const expectedNode = readFileSync(new URL("../.node-version", import.meta.url), "utf8").trim();
const workspaceManifest = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
);
const expectedPackageManager = workspaceManifest.packageManager;
const expectedPackageManagerUserAgent = expectedPackageManager.replace("@", "/");
const actualPackageManager = process.env.npm_config_user_agent?.split(" ", 1)[0];

const mismatches = [];

if (process.versions.node !== expectedNode) {
  mismatches.push(`Node ${expectedNode} is required; found ${process.versions.node}.`);
}

if (actualPackageManager !== expectedPackageManagerUserAgent) {
  mismatches.push(
    `${expectedPackageManager} is required; found ${actualPackageManager ?? "an unknown package manager"}.`,
  );
}

if (mismatches.length > 0) {
  for (const mismatch of mismatches) {
    console.error(`[ring3 toolchain] ${mismatch}`);
  }

  process.exitCode = 1;
}
