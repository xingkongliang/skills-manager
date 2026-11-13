import ReactMarkdown from "react-markdown";
import { useEffect, useMemo, useState } from "react";
import { cn } from "../utils";
import { markdownPlugins } from "../lib/markdown/config";
import { buildTranslationPlan, readerRemarkPlugin, type TranslationPlan } from "../lib/markdown/translationPlan";
import { classifyReaderLink, imageBlobUrl } from "../lib/skillReader";
import { readSkillFile, resolveSkillLink } from "../lib/tauri";

interface SkillReaderMarkdownProps {
  content: string;
  className?: string;
  plan?: TranslationPlan;
  translations?: Readonly<Record<string, string>>;
  onNavigate?: (href: string) => void;
  sessionId?: string;
  relativePath?: string;
}

// remark-gfm's autolink step compiles a regex with lookbehind on every render.
// WebKit before Safari 16.4 (e.g. an un-updated macOS Monterey) rejects that
// with a SyntaxError, which unmounts the whole app and leaves a blank window —
// the symptom reported in #613. Such engines get plain Markdown instead: no
// tables, task lists or strikethrough, but the skill is readable.
function ReaderImage({ src, alt, sessionId, relativePath }: { src?: string; alt?: string; sessionId?: string; relativePath?: string }) {
  const remote = src && /^https?:\/\//i.test(src);
  const [local, setLocal] = useState<{ src: string; url: string } | null>(null);
  useEffect(() => {
    if (!src || remote || !sessionId || !relativePath || classifyReaderLink(src) !== "internal") return;
    let live = true; let blob: string | null = null;
    void resolveSkillLink(sessionId, relativePath, src).then((link) => readSkillFile(sessionId, link.relative_path)).then((file) => {
      if (file.kind !== "image" || !file.base64) return;
      blob = imageBlobUrl(file.base64, file.mime);
      if (live) setLocal({ src, url: blob }); else URL.revokeObjectURL(blob);
    }).catch(() => {});
    return () => { live = false; if (blob) URL.revokeObjectURL(blob); };
  }, [src, remote, sessionId, relativePath]);
  const url = remote ? src : local && local.src === src ? local.url : undefined;
  return url ? <img src={url} alt={alt ?? ""} loading="lazy" referrerPolicy="no-referrer" className="my-3 max-h-[640px] max-w-full rounded-xl object-contain" /> : <span className="text-[12px] text-muted">{alt || src}</span>;
}

export function SkillReaderMarkdown({ content, className, plan: suppliedPlan, translations, onNavigate, sessionId, relativePath }: SkillReaderMarkdownProps) {
  const plan = useMemo(() => suppliedPlan ?? buildTranslationPlan(content), [content, suppliedPlan]);
  const plugins = useMemo(() => [...markdownPlugins, readerRemarkPlugin(plan, translations)], [plan, translations]);

  return (
    <article className={cn("mx-auto w-full max-w-[1240px] break-words text-[13px] leading-6 text-secondary", className)}>
      <ReactMarkdown
        remarkPlugins={plugins}
        components={{
          h1: ({ className, ...props }) => (
            <h1
              className={cn("mb-4 text-[28px] font-semibold leading-tight text-primary", className)}
              {...props}
            />
          ),
          h2: ({ className, ...props }) => (
            <h2
              className={cn("mb-3 mt-8 text-[20px] font-semibold leading-tight text-primary", className)}
              {...props}
            />
          ),
          h3: ({ className, ...props }) => (
            <h3
              className={cn("mb-2 mt-6 text-[16px] font-semibold leading-tight text-primary", className)}
              {...props}
            />
          ),
          p: ({ className, ...props }) => (
            <p className={cn("mb-4 text-[13px] leading-6 text-secondary", className)} {...props} />
          ),
          a: ({ className, href, ...props }) => {
            const dangerous = /^(javascript|vbscript|data):/i;
            const safeHref = href && !dangerous.test(href.trim()) ? href : undefined;
            return (
              <a
                className={cn("text-accent-light underline decoration-accent-border underline-offset-4", className)}
                href={safeHref}
                target="_blank"
                rel="noreferrer"
                onClick={onNavigate ? (event) => { event.preventDefault(); if (href) onNavigate(href); } : undefined}
                {...props}
              />
            );
          },
          img: ({ src, alt }) => <ReaderImage key={src} src={src} alt={alt} sessionId={sessionId} relativePath={relativePath} />,
          ul: ({ className, ...props }) => (
            <ul className={cn("mb-4 list-disc space-y-1 pl-5 text-secondary", className)} {...props} />
          ),
          ol: ({ className, ...props }) => (
            <ol className={cn("mb-4 list-decimal space-y-1 pl-5 text-secondary", className)} {...props} />
          ),
          li: ({ className, ...props }) => (
            <li className={cn("pl-1 marker:text-muted", className)} {...props} />
          ),
          blockquote: ({ className, ...props }) => (
            <blockquote
              className={cn(
                "mb-4 border-l-2 border-accent-border bg-surface/70 px-4 py-2 text-tertiary italic",
                className
              )}
              {...props}
            />
          ),
          hr: ({ className, ...props }) => (
            <hr className={cn("my-6 border-border-subtle", className)} {...props} />
          ),
          code: ({ className, children, ...props }) => {
            const isBlock = String(className || "").includes("language-");
            if (isBlock) {
              return (
                <code className={cn("block text-[13px] leading-6 text-secondary", className)} {...props}>
                  {children}
                </code>
              );
            }

            return (
              <code
                className={cn(
                  "rounded bg-surface-hover px-1.5 py-0.5 font-mono text-[13px] text-accent-light",
                  className
                )}
                {...props}
              >
                {children}
              </code>
            );
          },
          pre: ({ className, ...props }) => (
            <pre
              className={cn(
                "mb-4 overflow-x-auto rounded-xl border border-border-subtle bg-background px-4 py-3",
                className
              )}
              {...props}
            />
          ),
          table: ({ className, ...props }) => (
            <div className="mb-4 overflow-x-auto rounded-xl border border-border-subtle">
              <table className={cn("min-w-full border-collapse text-left text-[13px]", className)} {...props} />
            </div>
          ),
          thead: ({ className, ...props }) => (
            <thead className={cn("bg-surface-hover text-primary", className)} {...props} />
          ),
          th: ({ className, ...props }) => (
            <th className={cn("border-b border-border-subtle px-3 py-2 font-medium", className)} {...props} />
          ),
          td: ({ className, ...props }) => (
            <td className={cn("border-b border-border-subtle px-3 py-2 text-secondary", className)} {...props} />
          ),
        }}
      >
        {plan.markdown}
      </ReactMarkdown>
    </article>
  );
}
