// Minimal SSE helper for dashboard
// Subscribes to EventSource '/api/events' and invokes the provided callback
// whenever the connection opens (including reconnects). Returns an unsubscribe
// function that closes the EventSource.

export function subscribeToSseOpen(onOpen: () => void): () => void {
  const es = new EventSource('/api/events');

  const handleOpen = () => {
    try {
      onOpen();
    } catch {
      // swallow callback errors to avoid tearing down the SSE session
    }
  };

  es.addEventListener('open', handleOpen);

  // noop error handler to avoid noisy console in case of transient disconnects
  const handleError = () => {
    // EventSource will auto-reconnect; keep silent here
  };
  es.addEventListener('error', handleError);

  return () => {
    es.removeEventListener('open', handleOpen as EventListener);
    es.removeEventListener('error', handleError as EventListener);
    es.close();
  };
}
