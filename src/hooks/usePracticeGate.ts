import { useCallback, useEffect, useRef, useState } from "react";
import { practiceBegin, practiceEnd } from "@/lib/api";

/**
 * Drives the engine practice gate (practiceBegin/End, which suppresses ambient
 * stats + the coaching overlay) from BOTH the drill input's focus AND the app
 * window's focus.
 *
 * Why the window matters: switching to another OS app does NOT fire a DOM
 * `blur` on the focused input — the element keeps document focus; only the
 * window deactivates. Tying the gate to input blur alone left it ON while the
 * user was away in another app, so coaching stayed suppressed everywhere until
 * they navigated routes. Watching `window` focus/blur releases the gate the
 * moment the app loses focus and re-acquires it when both are focused again.
 *
 * `currentPhrase` is the phrase to pass to practiceBegin; it's read live so
 * advancing cards mid-drill doesn't need to re-acquire the gate.
 */
export function usePracticeGate(currentPhrase: string | undefined) {
  const [inputFocused, setInputFocused] = useState(false);
  const [windowFocused, setWindowFocused] = useState(
    typeof document !== "undefined" ? document.hasFocus() : true,
  );
  const phraseRef = useRef(currentPhrase);
  phraseRef.current = currentPhrase;
  const gateOnRef = useRef(false);

  const setGate = useCallback((on: boolean) => {
    if (on && !gateOnRef.current && phraseRef.current) {
      gateOnRef.current = true;
      void practiceBegin(phraseRef.current).catch(() => undefined);
    } else if (!on && gateOnRef.current) {
      gateOnRef.current = false;
      void practiceEnd().catch(() => undefined);
    }
  }, []);

  // Window focus fires on OS app switch (unlike the input's blur).
  useEffect(() => {
    const onFocus = () => setWindowFocused(true);
    const onBlur = () => setWindowFocused(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  useEffect(() => {
    setGate(inputFocused && windowFocused);
  }, [inputFocused, windowFocused, setGate]);

  // Safety net: release the gate if the component unmounts while held.
  useEffect(() => () => setGate(false), [setGate]);

  return {
    /** True only when the drill is genuinely active (input AND window focused). */
    active: inputFocused && windowFocused,
    onInputFocus: useCallback(() => setInputFocused(true), []),
    onInputBlur: useCallback(() => setInputFocused(false), []),
  };
}
