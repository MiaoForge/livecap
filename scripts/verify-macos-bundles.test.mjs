import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { verifyMacosBundles } from "./verify-macos-bundles.mjs";

const roots = [];
afterEach(() =>
  roots
    .splice(0)
    .forEach((root) => rmSync(root, { recursive: true, force: true })),
);

describe.skipIf(process.platform !== "darwin")(
  "macOS packaged signatures",
  () => {
    it("rejects a linker-only signature and verifies the app, DMG and updater after bundle signing", () => {
      const root = mkdtempSync(join(tmpdir(), "livecap-signature-test-"));
      roots.push(root);
      const app = join(root, "macos/Livecap.app");
      mkdirSync(join(app, "Contents/MacOS"), { recursive: true });
      mkdirSync(join(app, "Contents/Resources"));
      mkdirSync(join(root, "dmg"));
      const source = join(root, "main.c");
      writeFileSync(source, "int main(void) { return 0; }\n");
      execFileSync("clang", [
        source,
        "-o",
        join(app, "Contents/MacOS/livecap"),
      ]);
      writeFileSync(
        join(app, "Contents/Info.plist"),
        `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>livecap</string>
<key>CFBundleIdentifier</key><string>com.chenmiaoi.livecap.signature-test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>`,
      );
      const resource = join(app, "Contents/Resources/test.txt");
      writeFileSync(resource, "original");
      expect(() => verifyMacosBundles(root)).toThrow();
      execFileSync("codesign", ["--force", "--sign", "-", app], {
        stdio: "pipe",
      });
      execFileSync(
        "hdiutil",
        [
          "create",
          "-srcfolder",
          app,
          "-volname",
          "Signature test",
          join(root, "dmg/test.dmg"),
        ],
        { stdio: "pipe" },
      );
      execFileSync("tar", [
        "-czf",
        join(root, "macos/Livecap.app.tar.gz"),
        "-C",
        join(root, "macos"),
        "Livecap.app",
      ]);
      expect(() => verifyMacosBundles(root, true)).not.toThrow();
      // Keep the standalone app valid, but corrupt only the copy in the updater archive.
      copyFileSync(resource, join(root, "original.txt"));
      writeFileSync(resource, "tampered");
      execFileSync("tar", [
        "-czf",
        join(root, "macos/Livecap.app.tar.gz"),
        "-C",
        join(root, "macos"),
        "Livecap.app",
      ]);
      copyFileSync(join(root, "original.txt"), resource);
      expect(() => verifyMacosBundles(root, true)).toThrow();
    }, 30_000);
  },
);
