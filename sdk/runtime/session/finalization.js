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
  const finishing = session.finalizing;
  void finishing.catch(() => {if (session.finalizing === finishing) session.finalizing = null;});
  return session.finalizing;
}
