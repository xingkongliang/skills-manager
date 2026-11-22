import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { diffExceedsBudget } from "../lib/diffBudget";

interface DocumentDiffViewerProps {
  original: string;
  updated: string;
  className?: string;
}

type DiffRow =
  | { type: "context"; leftNumber: number; rightNumber: number; leftContent: string; rightContent: string }
  | { type: "removed"; leftNumber: number; rightNumber: null; leftContent: string; rightContent: "" }
  | { type: "added"; leftNumber: null; rightNumber: number; leftContent: ""; rightContent: string };

interface DiffHunk {
  id: string;
  leftStart: number;
  leftCount: number;
  rightStart: number;
  rightCount: number;
  rows: DiffRow[];
}

const CONTEXT_LINES = 3;

function buildDiffRows(original: string, updated: string): DiffRow[] {
  const left = original.split("\n");
  const right = updated.split("\n");
  const dp = Array.from({ length: left.length + 1 }, () => Array<number>(right.length + 1).fill(0));

  for (let i = left.length - 1; i >= 0; i -= 1) {
    for (let j = right.length - 1; j >= 0; j -= 1) {
      if (left[i] === right[j]) {
        dp[i][j] = dp[i + 1][j + 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
  }

  const rows: DiffRow[] = [];
  let i = 0;
  let j = 0;
  let leftNumber = 1;
  let rightNumber = 1;

  while (i < left.length && j < right.length) {
    if (left[i] === right[j]) {
      rows.push({
        type: "context",
        leftNumber,
        rightNumber,
        leftContent: left[i],
        rightContent: right[j],
      });
      i += 1;
      j += 1;
      leftNumber += 1;
      rightNumber += 1;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      rows.push({
        type: "removed",
        leftNumber,
        rightNumber: null,
        leftContent: left[i],
        rightContent: "",
      });
      i += 1;
      leftNumber += 1;
    } else {
      rows.push({
        type: "added",
        leftNumber: null,
        rightNumber,
        leftContent: "",
        rightContent: right[j],
      });
      j += 1;
      rightNumber += 1;
    }
  }

  while (i < left.length) {
    rows.push({
      type: "removed",
      leftNumber,
      rightNumber: null,
      leftContent: left[i],
      rightContent: "",
    });
    i += 1;
    leftNumber += 1;
  }

  while (j < right.length) {
    rows.push({
      type: "added",
      leftNumber: null,
      rightNumber,
      leftContent: "",
      rightContent: right[j],
    });
    j += 1;
    rightNumber += 1;
  }

  return rows;
}

function buildHunks(rows: DiffRow[]): DiffHunk[] {
  const changedIndexes = rows
    .map((row, index) => ({ row, index }))
    .filter(({ row }) => row.type !== "context")
    .map(({ index }) => index);

  if (changedIndexes.length === 0) return [];

  const hunks: DiffHunk[] = [];
  let start = Math.max(0, changedIndexes[0] - CONTEXT_LINES);
  let end = Math.min(rows.length - 1, changedIndexes[0] + CONTEXT_LINES);

  for (let i = 1; i < changedIndexes.length; i += 1) {
    const nextStart = Math.max(0, changedIndexes[i] - CONTEXT_LINES);
    const nextEnd = Math.min(rows.length - 1, changedIndexes[i] + CONTEXT_LINES);
    if (nextStart <= end + 1) {
      end = Math.max(end, nextEnd);
    } else {
      hunks.push(createHunk(rows, start, end, hunks.length));
      start = nextStart;
      end = nextEnd;
    }
  }

  hunks.push(createHunk(rows, start, end, hunks.length));
  return hunks;
}

function createHunk(rows: DiffRow[], start: number, end: number, index: number): DiffHunk {
  const hunkRows = rows.slice(start, end + 1);
  const leftNumbers = hunkRows.flatMap((row) => (row.leftNumber == null ? [] : [row.leftNumber]));
  const rightNumbers = hunkRows.flatMap((row) => (row.rightNumber == null ? [] : [row.rightNumber]));

  return {
    id: `hunk-${index}-${start}-${end}`,
    leftStart: leftNumbers[0] ?? 0,
    leftCount: leftNumbers.length,
    rightStart: rightNumbers[0] ?? 0,
    rightCount: rightNumbers.length,
    rows: hunkRows,
  };
}

function cellTone(type: DiffRow["type"], side: "left" | "right") {
  if (type === "removed" && side === "left") {
    return {
      lineNoClass: "bg-[#ffd7d5] text-red-900 dark:bg-[#3c1618] dark:text-red-200",
      codeClass: "bg-[#ffebe9] text-red-950 dark:bg-[#2d1013] dark:text-red-50",
      codeStyle: { boxShadow: "inset 3px 0 0 #cf222e" },
      markerClass: "text-red-700 dark:text-red-300",
    };
  }
  if (type === "added" && side === "right") {
    return {
      lineNoClass: "bg-[#aceebb] text-emerald-900 dark:bg-[#1b4721] dark:text-emerald-200",
      codeClass: "bg-[#dafbe1] text-emerald-950 dark:bg-[#12261e] dark:text-emerald-50",
      codeStyle: { boxShadow: "inset 3px 0 0 #1a7f37" },
      markerClass: "text-emerald-700 dark:text-emerald-300",
    };
  }
  return {
    lineNoClass: "bg-surface-hover text-faint",
    codeClass: "bg-bg-secondary text-secondary",
    codeStyle: undefined,
    markerClass: "text-faint",
  };
}

function DiffCell({
  number,
  content,
  type,
  side,
}: {
  number: number | null;
  content: string;
  type: DiffRow["type"];
  side: "left" | "right";
}) {
  const tone = cellTone(type, side);
  const marker = side === "left" ? (type === "removed" ? "-" : " ") : (type === "added" ? "+" : " ");

  return (
    <>
      <td
        className={cn("w-14 select-none border-r border-border-subtle px-3 text-right font-mono text-[12px]", tone.lineNoClass)}
      >
        {number ?? ""}
      </td>
      <td
        className={cn("border-r border-border-subtle px-3 font-mono text-[13px] leading-6", tone.codeClass)}
        style={tone.codeStyle}
      >
        <span className={cn("mr-3 inline-block w-3 select-none text-center font-semibold", tone.markerClass)}>
          {marker}
        </span>
        <span className="whitespace-pre-wrap break-words">{content || " "}</span>
      </td>
    </>
  );
}

export function DocumentDiffViewer({ original, updated, className }: DocumentDiffViewerProps) {
  const { t } = useTranslation();
  const tooLarge = useMemo(() => diffExceedsBudget(original, updated), [original, updated]);
  const rows = useMemo(
    () => (tooLarge ? [] : buildDiffRows(original, updated)),
    [tooLarge, original, updated]
  );
  const hunks = useMemo(() => buildHunks(rows), [rows]);

  if (tooLarge) {
    return (
      <div className={cn("px-4 py-4 text-[12px] text-muted", className)}>
        {t("common.diffTooLarge")}
      </div>
    );
  }

  if (hunks.length === 0) {
    return (
      <div className={cn("rounded-xl border border-border-subtle bg-bg-secondary px-4 py-6 text-center", className)}>
        <div className="text-[13px] font-medium text-secondary">No content changes</div>
      </div>
    );
  }

  return (
    <div className={cn("space-y-4", className)}>
      {hunks.map((hunk) => (
        <div key={hunk.id} className="overflow-hidden rounded-xl border border-border-subtle bg-bg-secondary">
          <div className="grid grid-cols-2 border-b border-border-subtle bg-[#ddf4ff] dark:bg-[#0c2d40]">
            <div className="border-r border-border-subtle px-3 py-2 font-mono text-[11px] text-sky-800 dark:text-sky-300">
              @@ -{hunk.leftStart},{hunk.leftCount}
            </div>
            <div className="px-3 py-2 font-mono text-[11px] text-sky-800 dark:text-sky-300">
              @@ +{hunk.rightStart},{hunk.rightCount}
            </div>
          </div>

          <div className="overflow-x-auto">
            <table className="min-w-full border-collapse">
              <tbody>
                {hunk.rows.map((row, index) => (
                  <tr key={`${hunk.id}-${index}`} className="border-b border-border-subtle/80 last:border-b-0">
                    <DiffCell
                      number={row.leftNumber}
                      content={row.leftContent}
                      type={row.type}
                      side="left"
                    />
                    <DiffCell
                      number={row.rightNumber}
                      content={row.rightContent}
                      type={row.type}
                      side="right"
                    />
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ))}
    </div>
  );
}
