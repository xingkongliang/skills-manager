import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "../utils";

interface SkillMarkdownProps {
  content: string;
  className?: string;
}

function stripMarkdownFrontmatter(content: string) {
  if (!content.startsWith("---\n")) return content;

  const end = content.indexOf("\n---\n", 4);
  if (end === -1) return content;

  return content.slice(end + 5).trimStart();
}

export function SkillMarkdown({ content, className }: SkillMarkdownProps) {
  const markdown = stripMarkdownFrontmatter(content);

  return (
    <article className={cn("mx-auto w-full max-w-[1240px] text-[13px] leading-6 text-secondary", className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
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
            const safeHref = href && /^https?:\/\//.test(href) ? href : undefined;
            return (
              <a
                className={cn("text-accent-light underline decoration-accent-border underline-offset-4", className)}
                href={safeHref}
                target="_blank"
                rel="noreferrer"
                {...props}
              />
            );
          },
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
        {markdown}
      </ReactMarkdown>
    </article>
  );
}
