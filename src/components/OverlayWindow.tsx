import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import {
  cancelTranslation,
  onTranslationEvent,
  translateSelection,
} from "../lib/api";
import { isTauriRuntime } from "../lib/tauri";
import { SAMPLE_LOOKUP } from "../data/sample";
import type {
  AppSettings,
  LookupLoadingPayload,
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
  const [loading, setLoading] = useState<LookupLoadingPayload | null>(null);
  const [translation, setTranslation] = useState<TranslationState | null>(null);
  const [pinned, setPinned] = useState(false);
  const currentRequest = useRef("");

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const cleanups = Promise.all([
      listen<LookupResult>("eneye://lookup", (event) => {
        setLoading(null);
        setTranslation(null);
        setLookup(event.payload);
      }),
      listen<LookupLoadingPayload>("eneye://lookup-loading", (event) => {
        setLookup(null);
        setTranslation(null);
        setLoading(event.payload);
      }),
      listen<SelectionPayload>("eneye://selection", (event) => {
        currentRequest.current = event.payload.requestId;
        setLoading(null);
        setLookup(null);
        setTranslation({
          requestId: event.payload.requestId,
          text: event.payload.text,
          status: "streaming",
          output: "",
        });
      }),
      onTranslationEvent((event) => {
        if (event.requestId !== currentRequest.current) return;
        setTranslation((current) => {
          if (!current) return current;
          switch (event.type) {
            case "delta":
              return {
                ...current,
                status: "streaming",
                output: current.output + event.text,
              };
            case "completed":
              return {
                ...current,
                status: "complete",
                output: event.text,
              };
            case "failed":
              return {
                ...current,
                status: "error",
                error: event.error,
              };
            case "cancelled":
              return {
                ...current,
                status: "error",
                error: "翻译已取消",
              };
            default:
              return current;
          }
        });
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

  async function hide() {
    await invoke("hide_overlay");
  }

  async function changePinned(value: boolean) {
    setPinned(value);
    await invoke("set_overlay_pinned", { pinned: value });
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
      <main className="overlay-stage">
        <OverlayCard
          key={lookup.normalizedWord}
          onClose={() => void hide()}
          onPinChange={(value) => void changePinned(value)}
          settings={settings}
          lookup={lookup}
        />
      </main>
    );
  }

  if (translation) {
    return (
      <main className="overlay-stage">
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

  return (
    <main className="overlay-stage">
      <article className="dictionary-card loading-card" aria-live="polite">
        <span className="loading-orbit" aria-hidden="true" />
        <strong>正在识别屏幕文字</strong>
        <span>
          {loading?.source === "ocr" ? "本机 OCR 处理中…" : "读取选区中…"}
        </span>
      </article>
    </main>
  );
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
  return (
    <article className="translation-card">
      <header className="translation-card-header">
        <div>
          <p className="eyebrow">AI TRANSLATION</p>
          <h2>
            {translation.status === "streaming"
              ? "正在翻译"
              : translation.status === "error"
                ? "翻译未完成"
                : "译文"}
          </h2>
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
          <p className="translation-result">
            {translation.output || "正在连接 AI 服务…"}
            {translation.status === "streaming" && (
              <span className="streaming-caret" aria-hidden="true" />
            )}
          </p>
        )}
      </div>
      {translation.status === "streaming" && (
        <button className="translation-stop" onClick={onCancel} type="button">
          <Icon name="pause" size={15} />
          停止翻译
        </button>
      )}
    </article>
  );
}
