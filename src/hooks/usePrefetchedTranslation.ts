import { useCallback, useEffect, useRef, useState } from "react";
import { translateSkillDocument } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

interface TranslationRequest {
  source: string;
  promise: Promise<string>;
  status: "pending" | "fulfilled" | "rejected";
}

export function usePrefetchedTranslation(source: string | null | undefined) {
  const [translation, setTranslation] = useState<{ source: string; content: string } | null>(null);
  const [visibleSource, setVisibleSource] = useState<string | null>(null);
  const [waitingSource, setWaitingSource] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ source: string; message: string } | null>(null);
  const requestRef = useRef<TranslationRequest | null>(null);
  const requestedSourceRef = useRef<string | null>(null);

  const beginTranslation = useCallback((nextSource: string) => {
    const existing = requestRef.current;
    if (
      existing?.source === nextSource
      && (existing.status === "pending" || existing.status === "fulfilled")
    ) {
      return existing.promise;
    }

    const promise = translateSkillDocument(nextSource);
    const request: TranslationRequest = {
      source: nextSource,
      promise,
      status: "pending",
    };
    requestRef.current = request;

    void promise.then(
      (content) => {
        request.status = "fulfilled";
        if (requestRef.current !== request) return;
        setTranslation({ source: nextSource, content });
        setWaitingSource((current) => (current === nextSource ? null : current));
        setFailure((current) => (current?.source === nextSource ? null : current));
      },
      (error) => {
        request.status = "rejected";
        if (requestRef.current !== request) return;
        setWaitingSource((current) => (current === nextSource ? null : current));
        if (requestedSourceRef.current === nextSource) {
          setFailure({
            source: nextSource,
            message: getErrorMessage(error, "AI translation failed"),
          });
        }
      }
    );

    return promise;
  }, []);

  useEffect(() => {
    if (!source) {
      requestRef.current = null;
      return;
    }

    void beginTranslation(source);
    return () => {
      if (requestRef.current?.source === source) {
        requestRef.current = null;
      }
    };
  }, [beginTranslation, source]);

  const translationAvailable = Boolean(source && translation?.source === source);
  const translationVisible = Boolean(source && visibleSource === source);
  const showingTranslation = translationVisible && translationAvailable;
  const activeFailure =
    translationVisible && failure?.source === source ? failure : null;

  const toggleTranslation = useCallback(() => {
    if (!source) return;
    if (translationVisible) {
      setVisibleSource(null);
      setWaitingSource(null);
      setFailure(null);
      requestedSourceRef.current = null;
      return;
    }

    requestedSourceRef.current = source;
    setVisibleSource(source);
    setFailure(null);
    if (!translationAvailable) {
      setWaitingSource(source);
      void beginTranslation(source).catch(() => {
        // Failure state is set by the shared request observer above.
      });
    }
  }, [beginTranslation, source, translationAvailable, translationVisible]);

  return {
    displayedDocument: showingTranslation ? translation?.content : source,
    showingTranslation,
    translationLoading: translationVisible && waitingSource === source,
    translationFailed: Boolean(activeFailure),
    translationError: activeFailure?.message ?? null,
    toggleTranslation,
  };
}
