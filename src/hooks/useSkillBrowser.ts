import { useEffect, useRef, useState } from "react";
import * as api from "../lib/tauri";
import { readerRequestId } from "../lib/skillReader";

export function useSkillBrowser(target: api.SkillBrowserTarget) {
  const identity = JSON.stringify(target);
  const [refresh, setRefresh] = useState(0);
  const [state, setState] = useState<{ identity: string; session: api.SkillBrowserSession | null; error: unknown; opening: boolean }>({ identity, session: null, error: null, opening: true });
  const current = useRef<api.SkillBrowserSession | null>(null);
  const currentIdentity = useRef(identity);

  useEffect(() => {
    let live = true;
    let settled = false;
    const requestId = readerRequestId();
    api.openSkillBrowser(JSON.parse(identity) as api.SkillBrowserTarget, requestId)
      .then((session) => {
        settled = true;
        if (!live) { void api.closeSkillBrowser(session.session_id).catch(() => {}); return; }
        const previous = current.current;
        current.current = session;
        currentIdentity.current = identity;
        setState({ identity, session, error: null, opening: false });
        if (previous) void api.closeSkillBrowser(previous.session_id).catch(() => {});
      })
      .catch((error: unknown) => { settled = true; if (live) setState({ identity, session: currentIdentity.current === identity ? current.current : null, error, opening: false }); });
    return () => { live = false; if (!settled) void api.cancelOpenSkillBrowser(requestId).catch(() => {}); };
  }, [identity, refresh]);

  useEffect(() => () => {
    if (current.current) void api.closeSkillBrowser(current.current.session_id).catch(() => {});
  }, []);

  return { ...state, session: state.identity === identity ? state.session : null, opening: state.identity !== identity || state.opening, reopen: () => { setState((s) => ({ ...s, opening: true, error: null })); setRefresh((v) => v + 1); } };
}
