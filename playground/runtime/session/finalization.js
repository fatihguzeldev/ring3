export function finishSession(session, observation, outcome, error) {
  session.cancel();
  session.finalizing ??= (async () => {
    try {
      await session.settled();
      return await observation?.finish(outcome, error);
    } finally {
      session.stop(false);
    }
  })();
  return session.finalizing;
}
