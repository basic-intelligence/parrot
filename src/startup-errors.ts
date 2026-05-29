function escapeHtml(value: string) {
  return value.replace(/[&<>"']/g, (char) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return entities[char] ?? char;
  });
}

function startupMessage(error: unknown) {
  if (error instanceof Error) {
    return error.stack || error.message;
  }

  return String(error);
}

function showStartupError(error: unknown) {
  const app = document.querySelector<HTMLElement>("#app");
  if (!app) return;

  const hasRenderedApp =
    app.children.length > 1 ||
    app.firstElementChild?.classList.contains("loading") === false;
  if (hasRenderedApp) return;

  app.innerHTML = `<pre class="error">Parrot startup error\n\n${escapeHtml(startupMessage(error))}</pre>`;
}

window.addEventListener("error", (event) => {
  showStartupError(event.error || event.message);
});

window.addEventListener("unhandledrejection", (event) => {
  showStartupError(event.reason);
});
