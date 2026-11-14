import { lazy, Suspense, useState } from "react";

/**
 * react-dev-inspector integration, dev-only.
 *
 * The inspector package and its component are loaded via a dynamic import that
 * is guarded by `import.meta.env.DEV`, so it is tree-shaken out of production
 * builds entirely.
 *
 * Usage: toggle with the default hotkey (Ctrl/Cmd + Shift + C) or click the
 * element after activating, then it opens the source in your local IDE.
 */
const InspectorImpl = lazy(async () => {
  const { Inspector } = await import("react-dev-inspector");
  return { default: Inspector };
});

export function DevInspector() {
  const [active, setActive] = useState(false);

  if (!import.meta.env.DEV) return null;

  return (
    <Suspense fallback={null}>
      <InspectorImpl active={active} onActiveChange={setActive} keys={['ctrl', 'c']}
        onInspectElement={({ codeInfo }) => {
          const { relativePath, lineNumber, columnNumber } = codeInfo;
          console.warn("11111111", `${relativePath}:${lineNumber}:${columnNumber}`);
          navigator.clipboard.writeText(`${relativePath}:${lineNumber}:${columnNumber}`);
        }} />
    </Suspense>
  );
}
