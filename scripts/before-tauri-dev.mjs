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

function start(command, args) {
  const child = spawn(command, args, { stdio: "inherit" });

  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => {
      child.kill(signal);
    });
  }

  child.on("exit", (code, signal) => {
    process.exit(code ?? (signal ? 1 : 0));
  });
}

if (isMac) {
  await run(npmCommand, ["run", "build:core:mac"]);
}

start(npmCommand, ["run", "dev:ui"]);
