import fs from "node:fs";
import { spawnSync } from "node:child_process";

const TAG_RULES = `Tag language rules:
- Use Chinese for general concepts (e.g. "前端", "状态管理", "代码审查", "测试")
- Keep well-known technical terms in English (e.g. "React", "Git", "TDD", "GraphQL", "WebSocket")
- NEVER mix Chinese and English in one tag (e.g. "API设计" is WRONG, use "API 设计" or "接口设计")

Tag format rules:
- Each tag must be 2-4 characters/words, no long phrases
- English terms use their canonical casing (e.g. "React" not "react", "GraphQL" not "graphql")
- No duplicate meanings: do not output both "API 设计" and "接口设计", pick one`;

const PROMPTS = {
  tag_skill: `You are an AI coding skill classification expert. Based on the following skill content, generate 3-5 concise tags.

${TAG_RULES}

Tags should describe the skill's purpose, applicable tools, or technical domain. Return only a JSON array like ["React", "前端", "状态管理"], nothing else.`,

  generate_scenario_prompt: `You are an AI coding assistant. Given the scenario name and enabled skills below, generate a JSON object with two fields:
- "prompt": a default prompt under 180 characters. It must start with skill names in [skill::name] format separated by commas, followed by a natural lead-in that invites the user to describe their task.
- "recipes": an array containing exactly one recipe per enabled skill. Each recipe object must have "name" (short, 2-4 words), "skillNames" (array of enabled skill names used by that recipe), and "prompt_template" (same [skill::name] format as prompt, under 180 characters).

Hard requirements:
- The natural language in "prompt", every recipe "name", and every recipe "prompt_template" MUST use the requested output language.
- Every skill referenced in any recipe MUST come from the enabled skills list.
- The number of recipes MUST exactly equal the number of enabled skills.
- Every recipe MUST reference exactly one enabled skill.
- Every recipe.skillNames array MUST contain exactly one skill name.
- Every enabled skill MUST appear in exactly one recipe: no missing skills, no duplicate coverage, and no extra skills.
- Each recipe.prompt_template must use exactly the same skills listed in that recipe's skillNames.
- Do NOT combine multiple skills into one recipe.

Return ONLY the JSON object, no markdown fences, no explanation. Example: {"prompt": "[skill::brainstorming], [skill::code-review] I need to...", "recipes": [{"name": "Quick Review", "skillNames": ["code-review"], "prompt_template": "[skill::code-review] Review briefly:"}]}`,

  create_scenario: `You are an AI coding assistant scenario planner. Based on the installed skills list below, suggest 2-5 scenario groupings. Each scenario should have: name (concise), description (one sentence), icon (one emoji), skillNames (array of skill names from the input). Return only a JSON array like [{"name":"...","description":"...","icon":"🎨","skillNames":["..."]}], nothing else.`,

  batch_tag_skills: `You are an AI coding skill classification expert. For EACH skill below, generate 3-5 concise tags.

${TAG_RULES}
- Use CONSISTENT tags across skills when they share the same concept (e.g. always "前端" not sometimes "前端开发")

Return ONLY a JSON object mapping each skill name to its tags array, like:
{"skill-name-1": ["React", "前端", "状态管理"], "skill-name-2": ["Git", "版本控制"]}
Nothing else.`,

  consolidate_tags: `You are an AI tag taxonomy expert. Given a list of existing tags from a coding skills library, consolidate them into 20-30 canonical tags by merging synonyms and overly-specific variants.

${TAG_RULES}

Rules:
- Merge synonyms into one canonical tag (e.g. "UI 审查"/"UI 规范"/"UI 优化"/"UI 重设计" → "UI 设计")
- Merge overly-specific variants into broader categories (e.g. "视觉升级"/"视觉层次"/"视觉层级"/"配色" → "视觉设计")
- Keep technology-specific tags that are genuinely distinct (e.g. "React" and "Next.js" stay separate)
- Every original tag MUST appear in exactly one mapping
- The canonical tag can be one of the original tags or a new name

Return ONLY a JSON object where keys are canonical tags and values are arrays of original tags that map to them:
{"UI 设计": ["UI 审查", "UI 规范", "UI 优化", "UI 重设计"], "React": ["React"], ...}
Nothing else.`,
};

function buildUserContent(task, payload) {
  switch (task) {
    case "tag_skill":
      return `Skill name: ${payload.skillName}\n\nSkill content:\n${payload.skillContent}`;
    case "generate_scenario_prompt":
      return `Scenario: ${payload.scenarioName}\nRequested output language: ${payload.outputLanguage || "zh"}\n\nEnabled skills (${(payload.skills || []).length} total):\n${(payload.skills || []).map((s) => {
        const desc = (s.description || "").trim();
        const truncated = desc.length > 80 ? desc.slice(0, 80) + "…" : desc;
        return truncated ? `- ${s.name}: ${truncated}` : `- ${s.name}`;
      }).join("\n")}${payload.retryFeedback ? `\n\nPrevious result was invalid.\n${payload.retryFeedback}` : ""}`;
    case "create_scenario": {
      const skillLines = payload.skills.map((s) => {
        const desc = (s.description || "No description").slice(0, 80);
        return `- ${s.name} [tags: ${(s.tags || []).join(", ") || "none"}]: ${desc}`;
      }).join("\n");
      const existingSection = payload.existingScenarios?.length
        ? `\n\nExisting scenarios (do NOT recreate these):\n${payload.existingScenarios.map((s) => `- ${s}`).join("\n")}`
        : "";
      return `All installed skills:\n${skillLines}${existingSection}`;
    }
    case "batch_tag_skills":
      return payload.skills
        .map((s) => `=== ${s.name} ===\n${s.content}`)
        .join("\n\n");
    case "consolidate_tags":
      return `All existing tags (${payload.tags.length} total):\n${payload.tags.join(", ")}`;
    default:
      throw new Error(`Unknown task: ${task}`);
  }
}

function extractJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    const match = text.match(/(\[[\s\S]*\]|\{[\s\S]*\})/);
    if (match) {
      try {
        return JSON.parse(match[1]);
      } catch {
        // ignore
      }
    }
  }
  return null;
}

/** Normalize a tag for dedup comparison: lowercase, remove all spaces */
function normalizeTag(tag) {
  return tag.toLowerCase().replace(/\s+/g, "");
}

/** Deduplicate tags by normalized form, keeping the first occurrence */
function deduplicateTags(tags) {
  const seen = new Set();
  return tags.filter((tag) => {
    const key = normalizeTag(tag);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function formatResult(task, rawText) {
  switch (task) {
    case "tag_skill": {
      const parsed = extractJson(rawText);
      if (Array.isArray(parsed)) return { tags: deduplicateTags(parsed) };
      return null;
    }
    case "generate_scenario_prompt": {
      const text = rawText.trim();
      const MAX_PROMPT = 180;
      const MIN_BREAK = 50;
      // Expected shape: { prompt: string, recipes: [{ name, skillNames, prompt_template }] }
      const parsed = extractJson(text);
      if (parsed && typeof parsed.prompt === "string") {
        const prompt = parsed.prompt.trim().slice(0, MAX_PROMPT);
        const recipes = Array.isArray(parsed.recipes)
          ? parsed.recipes
              .filter((r) =>
                r
                && typeof r.name === "string"
                && typeof r.prompt_template === "string"
                && Array.isArray(r.skillNames)
                && r.skillNames.every((name) => typeof name === "string")
              )
              .map((r) => ({
                name: r.name.trim(),
                skillNames: deduplicateTags(r.skillNames.map((name) => name.trim()).filter(Boolean)),
                prompt_template: r.prompt_template.trim().slice(0, MAX_PROMPT),
              }))
          : [];
        return { prompt, recipes };
      }
      // Fallback: treat as plain text (backward compatibility)
      if (text.length <= MAX_PROMPT) return { prompt: text };
      const cutoff = text.slice(0, MAX_PROMPT);
      const lastPunct = Math.max(
        cutoff.lastIndexOf("。"),
        cutoff.lastIndexOf("，"),
        cutoff.lastIndexOf("！"),
        cutoff.lastIndexOf("？"),
        cutoff.lastIndexOf("."),
        cutoff.lastIndexOf(","),
      );
      const breakPoint = lastPunct > MIN_BREAK ? lastPunct + 1 : MAX_PROMPT;
      return { prompt: text.slice(0, breakPoint) };
    }
    case "create_scenario": {
      const parsed = extractJson(rawText);
      if (Array.isArray(parsed)) return { scenarios: parsed };
      return null;
    }
    case "batch_tag_skills": {
      const parsed = extractJson(rawText);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const results = {};
        for (const [name, tags] of Object.entries(parsed)) {
          if (Array.isArray(tags)) {
            results[name] = deduplicateTags(tags);
          }
        }
        return { results };
      }
      return null;
    }
    case "consolidate_tags": {
      const parsed = extractJson(rawText);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return { mapping: parsed };
      }
      return null;
    }
    default:
      return null;
  }
}

async function runCodebuddy({ fullPrompt, task, config }) {
  let query;
  try {
    ({ query } = await import("@tencent-ai/agent-sdk"));
  } catch (err) {
    throw new Error(
      `Missing @tencent-ai/agent-sdk dependency. Run npm install to restore app dependencies. ${err.message || String(err)}`
    );
  }

  const envVars = { CODEBUDDY_API_KEY: config.apiKey };
  if (config.internetEnvironment) envVars.CODEBUDDY_INTERNET_ENVIRONMENT = config.internetEnvironment;
  if (config.codebuddyCodePath) {
    try {
      fs.accessSync(config.codebuddyCodePath, fs.constants.X_OK);
    } catch {
      throw new Error(`Configured CodeBuddy CLI path is not executable: ${config.codebuddyCodePath}`);
    }
    envVars.CODEBUDDY_CODE_PATH = config.codebuddyCodePath;
  } else {
    const probe = spawnSync("codebuddy", ["--version"], {
      stdio: "ignore",
    });
    if (probe.error || probe.status !== 0) {
      throw new Error(
        "CodeBuddy CLI is required by the Agent SDK but was not found in PATH. Install CodeBuddy Code or set CODEBUDDY_CODE_PATH."
      );
    }
  }

  const maxTurns = (task === "batch_tag_skills" || task === "consolidate_tags" || task === "create_scenario") ? 3 : 1;

  let resultText = "";
  const q = query({
    prompt: fullPrompt,
    options: {
      permissionMode: "plan",
      maxTurns,
      env: envVars,
    },
  });

  for await (const message of q) {
    if (message.type === "assistant") {
      for (const block of message.message.content) {
        if (block.type === "text") {
          resultText += block.text;
        }
      }
    }
  }

  return resultText;
}

function chatCompletionsUrl(baseUrl) {
  const trimmed = baseUrl.trim().replace(/\/+$/, "");
  if (trimmed.endsWith("/chat/completions")) return trimmed;
  return `${trimmed}/chat/completions`;
}

async function runOpenAiCompatible({ systemPrompt, userContent, config }) {
  const body = {
    model: config.model,
    messages: [
      { role: "system", content: systemPrompt },
      { role: "user", content: userContent },
    ],
    temperature: config.temperature,
    max_tokens: config.maxTokens,
  };

  let response;
  try {
    response = await fetch(chatCompletionsUrl(config.baseUrl), {
      method: "POST",
      headers: {
        "Authorization": `Bearer ${config.apiKey}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
  } catch (err) {
    throw new Error(`OpenAI-compatible network request failed: ${err.message || String(err)}`);
  }

  const text = await response.text();
  let parsed = null;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    // Keep the raw text for the error below.
  }

  if (!response.ok) {
    const message = parsed?.error?.message || parsed?.message || text || response.statusText;
    throw new Error(`OpenAI-compatible request failed (${response.status}): ${message}`);
  }

  const content = parsed?.choices?.[0]?.message?.content;
  if (typeof content !== "string" || !content.trim()) {
    throw new Error("OpenAI-compatible response did not include message content");
  }
  return content;
}

async function main() {
  let inputData = "";
  for await (const chunk of process.stdin) {
    inputData += chunk;
  }

  let input;
  try {
    input = JSON.parse(inputData);
  } catch {
    console.log(JSON.stringify({ ok: false, error: "Invalid JSON input" }));
    process.exit(1);
  }

  const {
    task,
    payload,
    provider = "codebuddy",
    apiKey,
    internetEnvironment,
    codebuddyCodePath,
    codebuddy,
    openaiCompatible,
  } = input;

  if (!PROMPTS[task]) {
    console.log(JSON.stringify({ ok: false, error: `Unknown task: ${task}` }));
    process.exit(1);
  }

  const systemPrompt = PROMPTS[task];
  const userContent = buildUserContent(task, payload);
  const fullPrompt = `${systemPrompt}\n\n${userContent}`;

  try {
    let resultText = "";
    if (provider === "codebuddy") {
      resultText = await runCodebuddy({
        fullPrompt,
        task,
        config: {
          apiKey: codebuddy?.apiKey || apiKey,
          internetEnvironment: codebuddy?.internetEnvironment || internetEnvironment,
          codebuddyCodePath: codebuddy?.codebuddyCodePath || codebuddyCodePath,
        },
      });
    } else if (provider === "openai_compatible") {
      resultText = await runOpenAiCompatible({
        systemPrompt,
        userContent,
        config: openaiCompatible,
      });
    } else {
      throw new Error(`Unsupported AI provider: ${provider}`);
    }

    const data = formatResult(task, resultText);
    if (data) {
      console.log(JSON.stringify({ ok: true, data }));
    } else {
      console.log(
        JSON.stringify({ ok: false, error: "Failed to parse AI response", raw: resultText })
      );
    }
  } catch (err) {
    console.log(JSON.stringify({ ok: false, error: err.message || String(err) }));
    process.exit(1);
  }
}

main();
