const events = new EventSource("./events");

events.addEventListener("reload", () => {
  window.location.reload();
});

events.addEventListener("reload-error", (event) => {
  const message = JSON.parse(event.data)?.error ?? "dashboard reload stopped";
  console.error(`Bureau development reload: ${message}`);
});
