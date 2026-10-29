export function finishSession(session, observation, outcome, error) {
  if(outcome==='failed')session.cancel();
  session.finalizing ??= (async () => {
    try {
      if(outcome!=='failed' && typeof session.prepareFinalization==='function')await session.prepareFinalization();
      session.cancel();
      await session.settled();
      return await observation?.finish(outcome, error);
    } finally {
      if(!session.abort?.signal.aborted)session.cancel();
      session.stop(false);
    }
  })();
  const finishing = session.finalizing;
  void finishing.catch(() => {if (session.finalizing === finishing) session.finalizing = null;});
  return session.finalizing;
}
