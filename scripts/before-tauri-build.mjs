import { spawn } from "node:child_process";

const isMac = process.platform === "darwin";
const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm";

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: "inherit" });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} ${args.join(" ")} exited with ${code ?? signal}`));
      }
    });
  });
}

if (isMac) {
  await run("swift", [
    "scripts/render-dmg-background.swift",
    "src-tauri/assets/dmg-background.png",
  ]);
  await run(npmCommand, ["run", "build:core:mac"]);
}

await run(npmCommand, ["run", "build:ui"]);
