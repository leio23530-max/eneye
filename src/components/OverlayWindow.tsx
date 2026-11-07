import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import {
  cancelTranslation,
  onTranslationEvent,
  translateSelection,
} from "../lib/api";
import { isTauriRuntime } from "../lib/tauri";
import { applyTranslationEvent } from "../lib/translation-state";
import { SAMPLE_LOOKUP } from "../data/sample";
import type {
  AppSettings,
  LookupResult,
  SelectionPayload,
  TranslationState,
} from "../types";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";
import { OverlayCard } from "./OverlayCard";

interface OverlayWindowProps {
  settings: AppSettings;
}

export function OverlayWindow({ settings }: OverlayWindowProps) {
  const preview = !isTauriRuntime();
  const [lookup, setLookup] = useState<LookupResult | null>(
    preview ? SAMPLE_LOOKUP : null,
  );
  const [translation, setTranslation] = useState<TranslationState | null>(null);
  const [pinned, setPinned] = useState(false);
  const currentRequest = useRef("");
  const stageRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const cleanups = Promise.all([
      listen<LookupResult>("eneye://lookup", (event) => {
        setTranslation(null);
        setLookup(event.payload);
      }),
      listen<SelectionPayload>("eneye://selection", (event) => {
        currentRequest.current = event.payload.requestId;
        setLookup(null);
        setTranslation({
          requestId: event.payload.requestId,
          text: event.payload.text,
          kind: "translate",
          status: "streaming",
          output: "",
        });
      }),
      onTranslationEvent((event) => {
        if (event.requestId !== currentRequest.current) return;
        setTranslation((current) => applyTranslationEvent(current, event));
      }),
    ]);
    return () => {
      cleanups.then((unlisten) => unlisten.forEach((dispose) => dispose()));
    };
  }, []);

  useEffect(() => {
    if (!translation) return;
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") void hide();
    }
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [translation]);

  useEffect(() => {
    if (!lookup || preview) return;
    void invoke("set_overlay_pinned", { pinned: false });
  }, [lookup?.normalizedWord, preview]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let observer: ResizeObserver | undefined;
    let unlistenMoved: (() => void) | undefined;
    const selector = ".dictionary-card, .translation-card";

    async function publishBounds() {
      if (disposed) return;
      const card = stageRef.current?.querySelector<HTMLElement>(selector);
      if (!card) {
        await invoke("set_overlay_card_bounds", { bounds: null });
        return;
      }
      const rect = card.getBoundingClientRect();
      const position = await getCurrentWindow().outerPosition();
      if (disposed) return;
      const scale = window.devicePixelRatio || 1;
      await invoke("set_overlay_card_bounds", {
        bounds: {
          x: Math.round(position.x + rect.left * scale),
          y: Math.round(position.y + rect.top * scale),
          width: Math.max(1, Math.round(rect.width * scale)),
          height: Math.max(1, Math.round(rect.height * scale)),
        },
      });
    }

    void publishBounds();
    const card = stageRef.current?.querySelector<HTMLElement>(selector);
    if (card) {
      observer = new ResizeObserver(() => void publishBounds());
      observer.observe(card);
    }
    getCurrentWindow()
      .onMoved(() => void publishBounds())
      .then((unlisten) => {
        if (disposed) unlisten();
        else unlistenMoved = unlisten;
      });
    return () => {
      disposed = true;
      observer?.disconnect();
      unlistenMoved?.();
      void invoke("set_overlay_card_bounds", { bounds: null });
    };
  }, [lookup?.normalizedWord, translation?.requestId]);

  async function hide() {
    await invoke("hide_overlay");
  }

  async function changePinned(value: boolean) {
    setPinned(value);
    await invoke("set_overlay_pinned", { pinned: value });
  }

  function explainWord(word: string) {
    const requestId = crypto.randomUUID();
    currentRequest.current = requestId;
    setLookup(null);
    setTranslation({
      requestId,
      text: word,
      kind: "explain",
      status: "streaming",
      output: "",
    });
    translateSelection({
      requestId,
      text: word,
      sourceLanguage: settings.sourceLanguage,
      targetLanguage: settings.targetLanguage,
      source: "uia",
      kind: "explain",
    }).catch((error) => {
      setTranslation((current) =>
        current
          ? { ...current, status: "error", error: String(error) }
          : current,
      );
    });
  }

  function retryTranslation() {
    if (!translation) return;
    const requestId = crypto.randomUUID();
    currentRequest.current = requestId;
    setTranslation({
      ...translation,
      requestId,
      status: "streaming",
      output: "",
      error: undefined,
    });
    translateSelection({
      requestId,
      text: translation.text,
      sourceLanguage: settings.sourceLanguage,
      targetLanguage: settings.targetLanguage,
      source: "uia",
      kind: translation.kind,
    }).catch((error) => {
      setTranslation((current) =>
        current
          ? { ...current, status: "error", error: String(error) }
          : current,
      );
    });
  }

  if (lookup) {
    return (
      <main className="overlay-stage" ref={stageRef}>
        <OverlayCard
          key={lookup.normalizedWord}
          onClose={() => void hide()}
          onExplain={settings.aiTranslationEnabled ? explainWord : undefined}
          onPinChange={(value) => void changePinned(value)}
          settings={settings}
          lookup={lookup}
        />
      </main>
    );
  }

  if (translation) {
    return (
      <main className="overlay-stage" ref={stageRef}>
        <TranslationOverlayCard
          onCancel={() => void cancelTranslation(translation.requestId)}
          onClose={() => void hide()}
          onRetry={retryTranslation}
          settings={settings}
          translation={translation}
        />
      </main>
    );
  }

  return <main className="overlay-stage" ref={stageRef} />;
}

export function TranslationOverlayCard({
  translation,
  settings,
  onClose,
  onCancel,
  onRetry,
}: {
  translation: TranslationState;
  settings: AppSettings;
  onClose: () => void;
  onCancel: () => void;
  onRetry: () => void;
}) {
  const explain = translation.kind === "explain";
  const action = explain ? "解释" : "翻译";
  const title =
    translation.status === "streaming"
      ? "正在" + action
      : translation.status === "error"
        ? action + "未完成"
        : translation.status === "cancelled"
          ? "已停止" + action
          : explain
            ? "解释"
            : "译文";
  return (
    <article className="translation-card">
      <header className="translation-card-header">
        <div>
          <p className="eyebrow">
            {explain ? "AI EXPLANATION" : "AI TRANSLATION"}
          </p>
          <h2>{title}</h2>
        </div>
        <div className="card-actions">
          <IconButton
            className="translation-close"
            icon="close"
            label="关闭卡片"
            onClick={onClose}
          />
        </div>
      </header>
      <div className="translation-card-body">
        <p className="translation-original">{translation.text}</p>
        <div className="translation-rule" />
        {translation.status === "error" ? (
          <div className="translation-error" role="alert">
            <Icon name="alert" size={18} />
            <span>{translation.error}</span>
            <button className="text-action" onClick={onRetry} type="button">
              重试
            </button>
          </div>
        ) : (
          <>
            <p className="translation-result">
              {translation.output ||
                (translation.status === "streaming"
                  ? "正在连接 AI 服务…"
                  : "已停止生成，没有收到内容。")}
              {translation.status === "streaming" && (
                <span className="streaming-caret" aria-hidden="true" />
              )}
            </p>
            {translation.status === "cancelled" && (
              <p className="translation-cancelled">
                已停止，保留已经生成的内容。
                <button className="text-action" onClick={onRetry} type="button">
                  重新{action}
                </button>
              </p>
            )}
          </>
        )}
      </div>
      {translation.status === "streaming" && (
        <button className="translation-stop" onClick={onCancel} type="button">
          <Icon name="pause" size={15} />
          停止{action}
        </button>
      )}
    </article>
  );
}
