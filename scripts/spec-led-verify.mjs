#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, realpathSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import {
  ActiveCheckLifecycle,
  CheckCatalogSchema,
  CheckSelectionSchema,
  RepositoryOverlaySchema,
  RetrospectiveSchema,
  SatisfactionReviewSchema,
  SelectionReviewSchema,
  validateActivationCompatibility,
  validateCheckSelection,
  validateSatisfactionReview,
  validateSelectionReview,
} from "./generated/check-lifecycle-runtime.mjs";

const CONFIG_PATH = ".spec-led/config.json";
const CONFIG_KEYS = ["baselineCommands", "exemptionPath", "openSpecSchema", "planningRoots", "receiptRoot", "schemaVersion", "verifier"];
const allow = (classification, repoRoot, details) => ({ ok: true, classification, ...(repoRoot ? { repoRoot } : {}), ...(details ?? {}) });
const reject = (code, message, nextAction) => ({ ok: false, code, message, ...(nextAction ? { nextAction } : {}) });
const CHANGE_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const GIT_SHA = /^[0-9a-f]{40}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
const TASK_MARKER = /^(\s*(?:[-*+]|\d+[.)])\s+)\[(?: |x|X)\]/gm;
const QUALITY_ROWS = {
  dependencies: [["Dependency review"]],
  securityOrAuth: [["Auth review"], ["Security scan"]],
  httpApi: [["HTTP API contract"]],
  ui: [["UI visual and accessibility evidence", "UI visual evidence"]],
  meaningfulBehavior: [["Test-first implementation"]],
  fallow: [["Fallow"]],
};

function isInside(parent, child) {
  const path = relative(parent, child);
  return path === "" || (!path.startsWith(`..${sep}`) && path !== ".." && !isAbsolute(path));
}

function discoverGitRoot(cwd) {
  const result = spawnSync("git", ["-C", cwd, "rev-parse", "--show-toplevel"], {
    encoding: "utf8",
    env: { ...process.env, LC_ALL: "C" },
  });
  if (result.status === 0) {
    try {
      return { kind: "git", root: realpathSync(result.stdout.trim()) };
    } catch {
      return { kind: "error" };
    }
  }
  const ordinaryNonGit = /^fatal: not a git repository \(or any of the parent directories\)(?:: [^\r\n]*)?\r?\n?$/i;
  if (result.status === 128 && !result.error && ordinaryNonGit.test(result.stderr ?? "")) return { kind: "non-git" };
  return { kind: "error" };
}

function validStringArray(value) {
  return Array.isArray(value) && value.length > 0 && value.every((item) => typeof item === "string" && item.length > 0) && new Set(value).size === value.length;
}

function validPlanningRoot(repoRoot, root) {
  if (isAbsolute(root) || root.split(/[\\/]+/).some((part) => part === "." || part === "..")) return false;
  const resolved = resolve(repoRoot, root);
  return resolved !== repoRoot && isInside(repoRoot, resolved);
}

function validConfig(repoRoot, config) {
  if (!config || typeof config !== "object" || Array.isArray(config)) return false;
  if (Object.keys(config).sort().join("\0") !== CONFIG_KEYS.join("\0")) return false;
  return config.schemaVersion === 1
    && ["spec-led", "spec-led-v2"].includes(config.openSpecSchema)
    && [config.verifier, config.receiptRoot, config.exemptionPath].every((value) => typeof value === "string" && validPlanningRoot(repoRoot, value))
    && validStringArray(config.planningRoots)
    && config.planningRoots.every((root) => validPlanningRoot(repoRoot, root))
    && validStringArray(config.baselineCommands);
}

function loadConfig(repoRoot) {
  const path = resolve(repoRoot, CONFIG_PATH);
  const committed = gitText(repoRoot, "HEAD", CONFIG_PATH);
  if (committed.error && !existsSync(path)) {
    return { error: reject("REPOSITORY_UNCONFIGURED", "This Git repository has no advisory spec-led configuration.", "/spec-led-init") };
  }

  try {
    const config = JSON.parse(committed.error ? readFileSync(path, "utf8") : committed.content);
    if (!validConfig(repoRoot, config)) throw new Error("invalid configuration");
    return { config, committed: !committed.error };
  } catch {
    return { error: reject("CONFIG_INVALID", "The advisory spec-led configuration is malformed. Repair .spec-led/config.json to use readiness reporting.") };
  }
}

function activationStatus(repoRoot, config) {
  if (config.openSpecSchema !== "spec-led-v2") return null;
  const required = [
    ".spec-led/check-lifecycle.json",
    ".spec-led/catalog/v1/catalog.json",
    ".spec-led/contracts/check-lifecycle/v1/schema.json",
    ".spec-led/contracts/v2/schema.json",
    ".spec-led/skill-index/v1/index.json",
    "openspec/schemas/spec-led-v2/schema.yaml",
    `${dirname(config.verifier)}/generated/check-lifecycle-runtime.mjs`,
  ];
  const missing = required.filter((path) => !existsSync(resolve(repoRoot, path)));
  if (missing.length) return reject("ACTIVATION_INCOMPATIBLE", `Option B activation is missing: ${missing.join(", ")}.`);
  try {
    const marker = parseJson(readFileSync(resolve(repoRoot, ".spec-led/check-lifecycle.json"), "utf8"));
    validateActivationCompatibility(marker, ActiveCheckLifecycle);
  } catch (error) {
    return reject("ACTIVATION_INCOMPATIBLE", error instanceof Error ? error.message : String(error));
  }
  return null;
}

function run(command, args, cwd) {
  return spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, LC_ALL: "C" },
    maxBuffer: 1024 * 1024,
  });
}

function git(repoRoot, args) {
  return run("git", ["-C", repoRoot, ...args], repoRoot);
}

function gitText(repoRoot, ref, path) {
  const result = git(repoRoot, ["show", `${ref}:${path}`]);
  return result.status === 0 ? { content: result.stdout } : { error: result };
}

function parseJson(content) {
  try {
    return JSON.parse(content);
  } catch {
    return null;
  }
}

function plainObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value, keys) {
  return plainObject(value) && Object.keys(value).sort().join("\0") === [...keys].sort().join("\0");
}

function nonEmpty(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function specificText(value) {
  return nonEmpty(value) && !/^(?:n\/?a|none|tbd|<.*>)$/i.test(value.trim());
}

function markdownFields(content, heading) {
  const marker = `## ${heading}`;
  const start = content.toLowerCase().indexOf(marker.toLowerCase());
  if (start < 0) return new Map();
  const remainder = content.slice(start + marker.length);
  const nextHeading = remainder.search(/^## /m);
  const section = nextHeading < 0 ? remainder : remainder.slice(0, nextHeading);
  return new Map(
    section.split(/\r?\n/).flatMap((line) => {
      const field = line.match(/^\s*-\s+([^:]+):\s*(.*?)\s*$/);
      return field ? [[field[1].trim(), field[2].trim()]] : [];
    }),
  );
}

function validDateTime(value) {
  return nonEmpty(value) && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z$/.test(value) && !Number.isNaN(Date.parse(value));
}

function validApprover(value) {
  return exactKeys(value, ["name", "email"]) && nonEmpty(value.name) && nonEmpty(value.email) && value.email.length >= 3;
}

function validExemption(value) {
  return exactKeys(value, ["schemaVersion", "confirmedAt", "confirmationSource", "approver", "reason"])
    && value.schemaVersion === 1
    && validDateTime(value.confirmedAt)
    && value.confirmationSource === "pi-interactive"
    && validApprover(value.approver)
    && nonEmpty(value.reason);
}

function validImpactDecision(value, mutation = false) {
  if (!plainObject(value)) return false;
  if (mutation) {
    if (value.optedIn === false) return exactKeys(value, ["optedIn", "notApplicableReason"]) && nonEmpty(value.notApplicableReason);
    return value.optedIn === true
      && exactKeys(value, ["optedIn", "approvalRef", "scope", "runtimeBudget", "fastTestCommand", "evidenceThreshold"])
      && [value.approvalRef, value.scope, value.runtimeBudget, value.fastTestCommand, value.evidenceThreshold].every(nonEmpty);
  }
  if (value.affected === true) return exactKeys(value, ["affected", "reason"]) && nonEmpty(value.reason);
  return value.affected === false && exactKeys(value, ["affected", "notAffectedReason"]) && nonEmpty(value.notAffectedReason);
}

function validImpactArea(value) {
  if (!plainObject(value)) return false;
  if (value.affected === true) {
    return exactKeys(value, ["affected", "reasons", "diagram"])
      && Array.isArray(value.reasons)
      && value.reasons.length > 0
      && value.reasons.every(nonEmpty)
      && nonEmpty(value.diagram);
  }
  return value.affected === false && exactKeys(value, ["affected", "notAffectedReason"]) && nonEmpty(value.notAffectedReason);
}

function validImpact(value) {
  const qualityKeys = ["dependencies", "securityOrAuth", "httpApi", "ui", "meaningfulBehavior", "fallow", "mutationTesting"];
  return exactKeys(value, ["schemaVersion", "architecture", "sequence", "quality"])
    && value.schemaVersion === 1
    && validImpactArea(value.architecture)
    && validImpactArea(value.sequence)
    && exactKeys(value.quality, qualityKeys)
    && qualityKeys.every((key) => validImpactDecision(value.quality[key], key === "mutationTesting"));
}

export function normalizeTasks(content) {
  return content.replace(TASK_MARKER, "$1[?]");
}

function digest(content, mode = "raw-v1") {
  const value = mode === "normalized-tasks-v1" ? normalizeTasks(content) : content;
  return createHash("sha256").update(value).digest("hex");
}

function safeRelativePath(path) {
  return nonEmpty(path)
    && !isAbsolute(path)
    && !path.split(/[\\/]+/).some((part) => part === "" || part === "." || part === "..");
}

function validateImpactAndQuality(config, changeRoot, impact, qualityPlan, readPath) {
  if (!validImpact(impact)) return reject("IMPACT_INVALID", "impact.json does not match contract version 1. Repair the declaration before readiness.");

  for (const area of [impact.architecture, impact.sequence]) {
    if (!area.affected) continue;
    if (!safeRelativePath(area.diagram) || !area.diagram.endsWith(".md") || readPath(`${changeRoot}/${area.diagram}`) === null) {
      return reject("DIAGRAM_MISSING", `Required diagram ${area.diagram} is missing from the change directory.`);
    }
  }

  if (!nonEmpty(qualityPlan)) return reject("ARTIFACT_MISSING", "quality-plan.md is required before readiness.");
  for (const command of config.baselineCommands) {
    if (!qualityPlan.includes(`\`${command}\``)) return reject("QUALITY_PLAN_INCONSISTENT", `quality-plan.md must include baseline command: ${command}`);
  }
  if (!/final fresh-context code review is required/i.test(qualityPlan)) {
    return reject("QUALITY_PLAN_INCONSISTENT", "quality-plan.md must require a final fresh-context code review.");
  }

  const rows = new Map();
  const qualityLabels = new Set([...Object.values(QUALITY_ROWS).flat(2), "Code simplification"]);
  for (const line of qualityPlan.split(/\r?\n/)) {
    if (!line.trim().startsWith("|") || /^\|\s*-+/.test(line)) continue;
    const cells = line.split("|").slice(1, -1).map((cell) => cell.trim());
    if (!qualityLabels.has(cells[0])) continue;
    if (cells.length !== 5) {
      return reject("QUALITY_PLAN_INCONSISTENT", `quality-plan.md ${cells[0]} must use the five-column quality-plan format.`);
    }
    rows.set(cells[0], cells);
  }
  for (const [qualityKey, labelGroups] of Object.entries(QUALITY_ROWS)) {
    const decision = impact.quality[qualityKey];
    const expected = decision.affected;
    const expectedReason = expected ? decision.reason : decision.notAffectedReason;
    for (const labels of labelGroups) {
      const row = labels.map((label) => rows.get(label)).find(Boolean);
      const command = row?.[3];
      const evidence = row?.[4];
      if (!row
        || row[1].toLowerCase() !== (expected ? "yes" : "no")
        || row[2] !== expectedReason
        || !specificText(row[2])
        || !specificText(evidence)
        || (expected && !specificText(command))) {
        return reject("QUALITY_PLAN_INCONSISTENT", `quality-plan.md must record ${labels[0]} as ${expected ? "yes" : "no"} with the impact reason, command or skill, and evidence.`);
      }
    }
  }

  const simplification = rows.get("Code simplification");
  const simplificationApplies = simplification?.[1].toLowerCase();
  if (!simplification
    || !["yes", "no"].includes(simplificationApplies)
    || !specificText(simplification[2])
    || !specificText(simplification[4])
    || (simplificationApplies === "yes" && !specificText(simplification[3]))) {
    return reject("QUALITY_PLAN_INCONSISTENT", "quality-plan.md must record Code simplification with applicability, a specific reason, command or skill, and evidence.");
  }

  const mutation = impact.quality.mutationTesting;
  const fields = markdownFields(qualityPlan, "Mutation testing");
  const approved = fields.get("Human approved")?.toLowerCase();
  if (mutation.optedIn) {
    const matching = [
      ["Approval reference", mutation.approvalRef],
      ["Scope", mutation.scope],
      ["Runtime budget", mutation.runtimeBudget],
      ["Fast test command", mutation.fastTestCommand],
      ["Evidence threshold", mutation.evidenceThreshold],
    ].every(([field, expected]) => fields.get(field) === expected && specificText(expected));
    if (approved !== "yes" || !matching) {
      return reject("QUALITY_PLAN_INCONSISTENT", "quality-plan.md mutation testing must match the approved impact scope, budget, command, and evidence threshold.");
    }
  } else if (approved !== "no" || fields.get("Not applicable reason") !== mutation.notApplicableReason || !specificText(mutation.notApplicableReason)) {
    return reject("QUALITY_PLAN_INCONSISTENT", "quality-plan.md must record mutation testing as not approved with the impact non-applicability reason.");
  }
  return null;
}

function listFiles(root) {
  const files = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) files.push(path);
    }
  };
  if (existsSync(root)) visit(root);
  return files;
}

function reviewedArtifactError(reviewedArtifacts, path, content) {
  const record = reviewedArtifacts.find((artifact) => artifact.path === path);
  if (!record) return `Review is missing artifact binding: ${path}`;
  return record.sha256 === digest(content) ? null : `Review artifact hash mismatch: ${path}`;
}

function reviewSourceError(review, changeRoot, readPath) {
  if (!safeRelativePath(review.sourceReport)) return `Review source path is invalid: ${review.sourceReport}`;
  const content = readPath(`${changeRoot}/${review.sourceReport}`);
  if (content === null) return `Review source is missing: ${review.sourceReport}`;
  return digest(content) === review.sourceReportSha256 ? null : `Review source hash mismatch: ${review.sourceReport}`;
}

function selectionBindingErrors(review, selectionContent, changeRoot, readPath) {
  const errors = [];
  for (const artifact of review.reviewedArtifacts) {
    if (!safeRelativePath(artifact.path)) {
      errors.push(`Reviewed artifact path is invalid: ${artifact.path}`);
      continue;
    }
    const content = artifact.path === "check-selection.json"
      ? selectionContent
      : readPath(`${changeRoot}/${artifact.path}`);
    if (content === null) errors.push(`Reviewed artifact is missing: ${artifact.path}`);
    else if (digest(content) !== artifact.sha256) errors.push(`Reviewed artifact hash mismatch: ${artifact.path}`);
  }
  const required = reviewedArtifactError(review.reviewedArtifacts, "check-selection.json", selectionContent);
  if (required) errors.push(required);
  const source = reviewSourceError(review, changeRoot, readPath);
  if (source) errors.push(source);
  return errors;
}

function contentContainsCheck(content, checkId) {
  const escaped = checkId.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(?:^|\\s)check:${escaped}(?=\\s|$)`, "m").test(content);
}

function localSchemaName(readPath, changeRoot, fallback) {
  return readPath(`${changeRoot}/.openspec.yaml`)?.match(/^schema:\s*([^\s#]+)\s*$/m)?.[1] ?? fallback;
}

function v2PlanningArtifacts(repoRoot, changeRoot, readPath, qualityPlan, tasks) {
  const selectionContent = readPath(`${changeRoot}/check-selection.json`);
  const selectionValue = parseJson(selectionContent);
  const selectionResult = CheckSelectionSchema.safeParse(selectionValue);
  if (!selectionResult.success) return { error: reject("CHECK_SELECTION_INVALID", "check-selection.json does not match the Check Selection contract.") };

  const reviewValue = parseJson(readPath(`${changeRoot}/selection-review.json`));
  const reviewResult = SelectionReviewSchema.safeParse(reviewValue);
  if (!reviewResult.success) return { error: reject("SELECTION_REVIEW_INVALID", "selection-review.json does not match the independent Selection Review contract.") };
  const selectionBindings = selectionBindingErrors(reviewResult.data, selectionContent, changeRoot, readPath);
  if (selectionBindings.length) return { error: reject("SELECTION_REVIEW_STALE", selectionBindings.join("\n")) };

  const catalogMajor = selectionResult.data.catalogVersion.split(".")[0];
  const catalogPath = resolve(repoRoot, `.spec-led/catalog/v${catalogMajor}/catalog.json`);
  const catalogValue = existsSync(catalogPath) ? parseJson(readFileSync(catalogPath, "utf8")) : null;
  const catalogResult = CheckCatalogSchema.safeParse(catalogValue);
  if (!catalogResult.success || catalogResult.data.catalogVersion !== selectionResult.data.catalogVersion) {
    return { error: reject("CATALOG_VERSION_UNRESOLVED", `Core catalog ${selectionResult.data.catalogVersion} is not installed.`) };
  }

  const overlays = [];
  for (const pin of selectionResult.data.overlays) {
    const overlayPath = resolve(repoRoot, `.spec-led/overlays/${pin.id}/${pin.version}.json`);
    const overlayValue = existsSync(overlayPath) ? parseJson(readFileSync(overlayPath, "utf8")) : null;
    const overlayResult = RepositoryOverlaySchema.safeParse(overlayValue);
    if (!overlayResult.success || overlayResult.data.id !== pin.id || overlayResult.data.version !== pin.version) {
      return { error: reject("OVERLAY_VERSION_UNRESOLVED", `Repository overlay ${pin.id}@${pin.version} is not installed.`) };
    }
    overlays.push(overlayResult.data);
  }

  try {
    validateCheckSelection(catalogResult.data, overlays, selectionResult.data);
    validateSelectionReview(selectionResult.data, reviewResult.data);
  } catch (error) {
    return { error: reject("CHECK_SELECTION_INVALID", error instanceof Error ? error.message : String(error)) };
  }
  if (reviewResult.data.verdict !== "accepted") {
    return { error: reject("SELECTION_REVIEW_REJECTED", "Selection Review must be accepted before planning readiness.") };
  }

  const missingPlan = selectionResult.data.dispositions
    .filter(({ checkId }) => !contentContainsCheck(qualityPlan, checkId))
    .map(({ checkId }) => checkId);
  const missingTasks = selectionResult.data.dispositions
    .filter(({ kind, checkId }) => kind !== "not-applicable" && !contentContainsCheck(tasks, checkId))
    .map(({ checkId }) => checkId);
  if (missingPlan.length || missingTasks.length) {
    return { error: reject("CHECK_SELECTION_INCONSISTENT", `Quality plan missing: ${missingPlan.join(", ") || "none"}; tasks missing: ${missingTasks.join(", ") || "none"}.`) };
  }
  return { catalog: catalogResult.data, overlays, selection: selectionResult.data, review: reviewResult.data };
}

function satisfactionBindingErrors(review, changeRoot, readPath) {
  const errors = [];
  for (const artifact of review.reviewedArtifacts) {
    if (!safeRelativePath(artifact.path)) {
      errors.push(`Reviewed artifact path is invalid: ${artifact.path}`);
      continue;
    }
    const content = readPath(`${changeRoot}/${artifact.path}`);
    if (content === null) errors.push(`Reviewed artifact is missing: ${artifact.path}`);
    else if (digest(content) !== artifact.sha256) errors.push(`Reviewed artifact hash mismatch: ${artifact.path}`);
  }
  const source = reviewSourceError(review, changeRoot, readPath);
  if (source) errors.push(source);
  for (const decision of review.decisions) {
    const evidence = decision.kind === "post-release" ? decision.supportingEvidence : decision.evidence;
    for (const reference of evidence) {
      if (/^https?:\/\//i.test(reference.uri)) continue;
      if (!safeRelativePath(reference.uri)) {
        errors.push(`Local evidence path is invalid: ${reference.uri}`);
        continue;
      }
      const content = readPath(`${changeRoot}/${reference.uri}`);
      if (content === null) errors.push(`Local evidence is missing: ${reference.uri}`);
      else if (digest(content) !== reference.sha256) errors.push(`Local evidence hash mismatch: ${reference.uri}`);
    }
  }
  return errors;
}

function v2PostImplementation(changeRoot, readPath, planning) {
  const satisfactionContent = readPath(`${changeRoot}/satisfaction-review.json`);
  const retrospectiveContent = readPath(`${changeRoot}/retrospective.json`);
  if (satisfactionContent === null) return { status: { complete: false, missing: ["satisfaction-review.json", "retrospective.json"] } };
  const satisfactionResult = SatisfactionReviewSchema.safeParse(parseJson(satisfactionContent));
  if (!satisfactionResult.success) return { error: reject("SATISFACTION_REVIEW_INVALID", "satisfaction-review.json does not match the Satisfaction Review contract.") };
  const bindingErrors = satisfactionBindingErrors(satisfactionResult.data, changeRoot, readPath);
  if (bindingErrors.length) return { error: reject("SATISFACTION_REVIEW_INVALID", bindingErrors.join("\n")) };
  try {
    validateSatisfactionReview(
      planning.catalog,
      planning.overlays,
      planning.selection,
      satisfactionResult.data,
      new Date().toISOString(),
    );
  } catch (error) {
    return { error: reject("SATISFACTION_REVIEW_INVALID", error instanceof Error ? error.message : String(error)) };
  }
  if (satisfactionResult.data.verdict !== "accepted") {
    return { status: { complete: false, verdict: "rejected", missing: ["accepted Satisfaction Review"] } };
  }
  if (retrospectiveContent === null) return { status: { complete: false, missing: ["retrospective.json"] } };
  const retrospectiveResult = RetrospectiveSchema.safeParse(parseJson(retrospectiveContent));
  if (!retrospectiveResult.success || retrospectiveResult.data.changeName !== planning.selection.changeName) {
    return { error: reject("RETROSPECTIVE_INVALID", "retrospective.json does not match the reviewed change.") };
  }
  return { status: { complete: true, postRelease: satisfactionResult.data.decisions.filter(({ kind }) => kind === "post-release") } };
}

function expectedArtifactsWorking(repoRoot, config, change) {
  const changeRoot = `openspec/changes/${change}`;
  const impactPath = resolve(repoRoot, changeRoot, "impact.json");
  const qualityPath = resolve(repoRoot, changeRoot, "quality-plan.md");
  const tasksPath = resolve(repoRoot, changeRoot, "tasks.md");
  const readPath = (path) => {
    const absolute = resolve(repoRoot, path);
    return isInside(repoRoot, absolute) && existsSync(absolute) ? readFileSync(absolute, "utf8") : null;
  };
  const impact = existsSync(impactPath) ? parseJson(readFileSync(impactPath, "utf8")) : null;
  const qualityPlan = existsSync(qualityPath) ? readFileSync(qualityPath, "utf8") : null;
  const tasks = existsSync(tasksPath) ? readFileSync(tasksPath, "utf8") : null;
  const validation = validateImpactAndQuality(config, changeRoot, impact, qualityPlan, readPath);
  if (validation) return { error: validation };

  const schemaName = localSchemaName(readPath, changeRoot, config.openSpecSchema);
  const planning = schemaName === "spec-led-v2"
    ? v2PlanningArtifacts(repoRoot, changeRoot, readPath, qualityPlan, tasks ?? "")
    : null;
  if (planning?.error) return planning;

  const fixed = [
    "proposal.md",
    "design.md",
    ...(schemaName === "spec-led-v2" ? ["check-selection.json", "selection-review.json", planning.review.sourceReport] : []),
    "quality-plan.md",
    "tasks.md",
    "impact.json",
  ].map((name) => `${changeRoot}/${name}`);
  const specsRoot = resolve(repoRoot, changeRoot, "specs");
  const specs = listFiles(specsRoot)
    .filter((path) => path.endsWith(`${sep}spec.md`))
    .map((path) => relative(repoRoot, path).split(sep).join("/"))
    .sort();
  const diagrams = [impact.architecture, impact.sequence].filter((area) => area.affected).map((area) => `${changeRoot}/${area.diagram}`);
  const paths = [...fixed, ...specs, ...diagrams].sort();
  if (!specs.length || paths.some((path) => readPath(path) === null)) {
    return { error: reject("ARTIFACT_MISSING", "Proposal, delta specs, design, selection, review, quality plan, tasks, and impact artifacts required by the pinned schema must exist before readiness.") };
  }
  return { paths, impact, schemaName, planning, readPath, changeRoot };
}

function readinessCandidate(repoRoot, change, paths) {
  const changeRoot = `openspec/changes/${change}`;
  const status = git(repoRoot, ["status", "--porcelain=v1", "-z", "--", changeRoot]);
  if (status.status !== 0) return { error: reject("GIT_OPERATION_FAILED", "Git could not verify that readiness artifacts are committed.") };
  if (status.stdout) return { error: reject("ARTIFACTS_UNCOMMITTED", `Commit all artifacts for ${change} before readiness confirmation.`) };

  const head = git(repoRoot, ["rev-parse", "--verify", "HEAD^{commit}"]);
  const artifactCommit = head.stdout.trim();
  if (head.status !== 0 || !GIT_SHA.test(artifactCommit)) {
    return { error: reject("ARTIFACTS_UNCOMMITTED", `Commit all artifacts for ${change} before readiness confirmation.`) };
  }

  const artifacts = [];
  for (const path of paths) {
    const committed = gitText(repoRoot, artifactCommit, path);
    if (committed.error) return { error: reject("ARTIFACTS_UNCOMMITTED", `Commit all artifacts for ${change} before readiness confirmation.`) };
    const hashMode = path.endsWith("/tasks.md") ? "normalized-tasks-v1" : "raw-v1";
    artifacts.push({ path, sha256: digest(committed.content, hashMode), hashMode });
  }
  return { artifactCommit, artifacts };
}

function expectedArtifactsAt(repoRoot, config, change, ref) {
  const changeRoot = `openspec/changes/${change}`;
  const impactResult = gitText(repoRoot, ref, `${changeRoot}/impact.json`);
  const qualityResult = gitText(repoRoot, ref, `${changeRoot}/quality-plan.md`);
  if (impactResult.error || qualityResult.error) return { error: reject("ARTIFACT_MISSING", "Receipt-bound change artifacts are missing from Git history.") };
  const impact = parseJson(impactResult.content);
  const readPath = (path) => {
    const result = gitText(repoRoot, ref, path);
    return result.error ? null : result.content;
  };
  const validation = validateImpactAndQuality(config, changeRoot, impact, qualityResult.content, readPath);
  if (validation) return { error: validation };

  const tree = git(repoRoot, ["ls-tree", "-r", "--name-only", ref, "--", `${changeRoot}/specs`]);
  if (tree.status !== 0) return { error: reject("GIT_OPERATION_FAILED", "Git could not inspect receipt-bound delta specifications.") };
  const specs = tree.stdout.split(/\r?\n/).filter((path) => path.endsWith("/spec.md")).sort();
  const schemaName = localSchemaName(readPath, changeRoot, config.openSpecSchema);
  const review = schemaName === "spec-led-v2"
    ? SelectionReviewSchema.safeParse(parseJson(readPath(`${changeRoot}/selection-review.json`)))
    : null;
  if (review && !review.success) return { error: reject("SELECTION_REVIEW_INVALID", "Receipt-bound Selection Review is invalid.") };
  const fixed = [
    "proposal.md",
    "design.md",
    ...(review?.success ? ["check-selection.json", "selection-review.json", review.data.sourceReport] : []),
    "quality-plan.md",
    "tasks.md",
    "impact.json",
  ].map((name) => `${changeRoot}/${name}`);
  const diagrams = [impact.architecture, impact.sequence].filter((area) => area.affected).map((area) => `${changeRoot}/${area.diagram}`);
  const paths = [...fixed, ...specs, ...diagrams].sort();
  if (!specs.length || paths.some((path) => readPath(path) === null)) {
    return { error: reject("ARTIFACT_MISSING", "Receipt-bound artifacts are incomplete in Git history.") };
  }
  return { paths, impact, schemaName };
}

function validReceiptShape(receipt, change) {
  const baseKeys = ["schemaVersion", "changeName", "approvedAt", "approvalSource", "approver", "artifactCommit", "artifacts", "taskHashNormalizationVersion"];
  const v2 = receipt?.schemaVersion === 2;
  const keys = v2 ? [...baseKeys, "catalogVersion", "overlayVersions"] : baseKeys;
  if (!exactKeys(receipt, keys)
    || ![1, 2].includes(receipt.schemaVersion)
    || receipt.changeName !== change
    || !CHANGE_NAME.test(receipt.changeName)
    || !validDateTime(receipt.approvedAt)
    || receipt.approvalSource !== "pi-interactive"
    || !validApprover(receipt.approver)
    || !GIT_SHA.test(receipt.artifactCommit)
    || receipt.taskHashNormalizationVersion !== 1
    || !Array.isArray(receipt.artifacts)
    || receipt.artifacts.length < (v2 ? 8 : 6)) return false;
  if (v2) {
    if (!SEMVER.test(receipt.catalogVersion) || !Array.isArray(receipt.overlayVersions)) return false;
    const pins = receipt.overlayVersions.map((pin) => exactKeys(pin, ["id", "version"])
      && CHANGE_NAME.test(pin.id)
      && SEMVER.test(pin.version)
      ? `${pin.id}@${pin.version}`
      : null);
    if (pins.includes(null) || new Set(pins).size !== pins.length) return false;
  }
  return receipt.artifacts.every((artifact) => exactKeys(artifact, ["path", "sha256", "hashMode"])
    && safeRelativePath(artifact.path)
    && SHA256.test(artifact.sha256)
    && ["raw-v1", "normalized-tasks-v1"].includes(artifact.hashMode));
}

function receiptPath(config, change) {
  return `${config.receiptRoot.replace(/\/+$/, "")}/${change}.json`;
}

function verifyReceipt(repoRoot, config, change, atCommit = "HEAD", checkWorking = true) {
  const path = receiptPath(config, change);
  const committed = gitText(repoRoot, atCommit, path);
  if (committed.error) return reject("RECEIPT_MISSING", `No committed readiness receipt exists for ${change}.`, `/spec-ready ${change}`);
  const receipt = parseJson(committed.content);
  if (!validReceiptShape(receipt, change)) return reject("RECEIPT_INVALID", `The committed readiness receipt for ${change} is malformed.`);

  if (checkWorking) {
    const workingPath = resolve(repoRoot, path);
    if (!existsSync(workingPath)) return reject("RECEIPT_STALE", `The working-tree readiness receipt for ${change} is missing.`);
    const working = readFileSync(workingPath, "utf8");
    if (!parseJson(working) || working !== committed.content) return reject("RECEIPT_INVALID", `The working-tree readiness receipt for ${change} differs from committed HEAD.`);
  }

  const ancestor = git(repoRoot, ["merge-base", "--is-ancestor", receipt.artifactCommit, atCommit]);
  if (ancestor.status !== 0) return reject("RECEIPT_INVALID", `The approval commit for ${change} is not an ancestor of ${atCommit}.`);
  const expected = expectedArtifactsAt(repoRoot, config, change, receipt.artifactCommit);
  if (expected.error) return expected.error;
  const expectedReceiptVersion = expected.schemaName === "spec-led-v2" ? 2 : 1;
  if (receipt.schemaVersion !== expectedReceiptVersion) {
    return reject("RECEIPT_INVALID", `Readiness receipt version ${receipt.schemaVersion} does not match schema ${expected.schemaName}.`);
  }
  if (expectedReceiptVersion === 2) {
    const selectionPath = `${`openspec/changes/${change}`}/check-selection.json`;
    const selection = parseJson(gitText(repoRoot, receipt.artifactCommit, selectionPath).content);
    const parsed = CheckSelectionSchema.safeParse(selection);
    const receiptPins = receipt.overlayVersions.map(({ id, version }) => `${id}@${version}`).sort();
    const selectionPins = parsed.success
      ? parsed.data.overlays.map(({ id, version }) => `${id}@${version}`).sort()
      : [];
    if (!parsed.success
      || receipt.catalogVersion !== parsed.data.catalogVersion
      || receiptPins.join("\0") !== selectionPins.join("\0")) {
      return reject("RECEIPT_INVALID", "Readiness receipt catalog and overlay pins do not match check-selection.json.");
    }
  }
  const byPath = new Map(receipt.artifacts.map((artifact) => [artifact.path, artifact]));
  if (byPath.size !== receipt.artifacts.length
    || expected.paths.length !== receipt.artifacts.length
    || expected.paths.some((artifactPath) => !byPath.has(artifactPath))) {
    return reject("RECEIPT_INVALID", `The readiness receipt for ${change} does not list exactly the required artifacts.`);
  }

  for (const artifactPath of expected.paths) {
    const record = byPath.get(artifactPath);
    const expectedMode = artifactPath.endsWith("/tasks.md") ? "normalized-tasks-v1" : "raw-v1";
    if (record.hashMode !== expectedMode) return reject("RECEIPT_INVALID", `The readiness receipt uses the wrong hash mode for ${artifactPath}.`);
    const approved = gitText(repoRoot, receipt.artifactCommit, artifactPath);
    if (approved.error || digest(approved.content, record.hashMode) !== record.sha256) {
      return reject("RECEIPT_INVALID", `The readiness receipt hash does not match approval commit artifact ${artifactPath}.`);
    }
    const current = gitText(repoRoot, atCommit, artifactPath);
    if (current.error || digest(current.content, record.hashMode) !== record.sha256) {
      return reject("RECEIPT_STALE", `Receipt-bound artifact ${artifactPath} changed after approval. Re-approve and commit a replacement receipt.`);
    }
    if (checkWorking) {
      const workingPath = resolve(repoRoot, artifactPath);
      if (!existsSync(workingPath) || digest(readFileSync(workingPath, "utf8"), record.hashMode) !== record.sha256) {
        return reject("RECEIPT_STALE", `Working-tree artifact ${artifactPath} differs from its committed readiness receipt.`);
      }
    }
  }
  return allow("ready", repoRoot, { change });
}

function verifyExemption(repoRoot, config, atCommit = "HEAD", checkWorking = true) {
  const committed = gitText(repoRoot, atCommit, config.exemptionPath);
  const workingPath = resolve(repoRoot, config.exemptionPath);
  if (committed.error) {
    if (checkWorking && existsSync(workingPath)) {
      const working = parseJson(readFileSync(workingPath, "utf8"));
      return validExemption(working)
        ? reject("EXEMPTION_UNCOMMITTED", "The advisory exemption is not committed, so readiness status cannot verify it against Git history.")
        : reject("EXEMPTION_INVALID", "The advisory exemption record is malformed.");
    }
    return null;
  }
  const value = parseJson(committed.content);
  if (!validExemption(value)) return reject("EXEMPTION_INVALID", "The committed exemption record is malformed.");
  if (checkWorking) {
    if (!existsSync(workingPath)) return reject("EXEMPTION_INVALID", "The committed exemption is missing from the working tree.");
    const working = readFileSync(workingPath, "utf8");
    if (working !== committed.content || !validExemption(parseJson(working))) return reject("EXEMPTION_INVALID", "The working exemption differs from committed HEAD.");
  }
  return allow("exempt", repoRoot, { reason: value.reason });
}

function receiptChangesAt(repoRoot, config, ref = "HEAD") {
  const root = config.receiptRoot.replace(/\/+$/, "");
  const tree = git(repoRoot, ["ls-tree", "-r", "--name-only", ref, "--", root]);
  if (tree.status !== 0) {
    const hasCommit = git(repoRoot, ["rev-parse", "--verify", `${ref}^{commit}`]);
    if (hasCommit.status !== 0) return { changes: [] };
    return { error: reject("GIT_OPERATION_FAILED", "Git could not inspect readiness receipts.") };
  }
  const suffix = ".json";
  const changes = tree.stdout.split(/\r?\n/)
    .filter((path) => path.startsWith(`${root}/`) && path.endsWith(suffix))
    .map((path) => path.slice(root.length + 1, -suffix.length));
  return { changes };
}

function repositoryReadiness(repoRoot, config, atCommit = "HEAD", checkWorking = true) {
  const exemption = verifyExemption(repoRoot, config, atCommit, checkWorking);
  if (exemption) return exemption;
  const listed = receiptChangesAt(repoRoot, config, atCommit);
  if (listed.error) return listed.error;
  if (!listed.changes.length) {
    const workingRoot = resolve(repoRoot, config.receiptRoot);
    if (checkWorking && existsSync(workingRoot) && listFiles(workingRoot).some((path) => path.endsWith(".json"))) {
      return reject("RECEIPT_UNCOMMITTED", "A working-tree readiness receipt is not committed; the advisory report cannot verify it against Git history.");
    }
    return reject("READINESS_REQUIRED", "No committed advisory readiness receipt is available.");
  }
  for (const change of listed.changes) {
    if (!CHANGE_NAME.test(change)) return reject("RECEIPT_INVALID", "A committed readiness receipt has an invalid change name.");
    const verified = verifyReceipt(repoRoot, config, change, atCommit, checkWorking);
    if (!verified.ok) return verified;
  }
  return allow("ready", repoRoot, { changes: listed.changes });
}

const advisoryDisabled = (mode) => allow("advisory-disabled", null, {
  mode,
  message: "Spec-led enforcement is disabled; ordinary local work is not gated by readiness state.",
});

export function verifyTool() {
  return advisoryDisabled("tool");
}

function openSpecValidation(repoRoot, config, change) {
  const changeRoot = resolve(repoRoot, "openspec", "changes", change);
  if (!existsSync(changeRoot)) return reject("CHANGE_NOT_FOUND", `OpenSpec change ${change} does not exist.`);
  const localSchemaPath = resolve(changeRoot, ".openspec.yaml");
  const localSchema = existsSync(localSchemaPath) ? readFileSync(localSchemaPath, "utf8").match(/^schema:\s*([^\s#]+)\s*$/m)?.[1] : config.openSpecSchema;
  const predecessor = config.openSpecSchema === "spec-led-v2" && localSchema === "spec-led";
  // This archived bootstrap change created spec-led before that schema could name itself.
  const bootstrap = change === "add-spec-led-mutation-guard" && localSchema === "spec-driven";
  if (localSchema !== config.openSpecSchema && !predecessor && !bootstrap) {
    return reject("OPENSPEC_SCHEMA_INVALID", `OpenSpec change ${change} must use schema ${config.openSpecSchema} or its supported predecessor.`);
  }
  for (const args of [["validate", change, "--strict"], ["schema", "validate", localSchema]]) {
    const result = run("openspec", args, repoRoot);
    if (result.status !== 0) return reject("OPENSPEC_VALIDATION_FAILED", `OpenSpec validation failed: openspec ${args.join(" ")}`);
  }
  return null;
}

export function verifyStatus({ cwd = process.cwd(), change } = {}) {
  const repository = discoverGitRoot(cwd);
  if (repository.kind === "non-git") return allow("non-git");
  if (repository.kind === "error") return reject("GIT_OPERATION_FAILED", "Git repository discovery failed.");
  const repoRoot = repository.root;
  const loaded = loadConfig(repoRoot);
  if (loaded.error) return loaded.error;
  const activation = activationStatus(repoRoot, loaded.config);
  if (activation) return activation;
  const exemption = verifyExemption(repoRoot, loaded.config);
  if (exemption) return exemption;
  if (change === undefined) return repositoryReadiness(repoRoot, loaded.config);
  if (!nonEmpty(change) || !CHANGE_NAME.test(change)) return reject("INPUT_INVALID", "Status mode requires a valid kebab-case change name.");

  const openSpec = openSpecValidation(repoRoot, loaded.config, change);
  if (openSpec) return openSpec;
  const artifacts = expectedArtifactsWorking(repoRoot, loaded.config, change);
  if (artifacts.error) return artifacts.error;
  const post = artifacts.schemaName === "spec-led-v2"
    ? v2PostImplementation(artifacts.changeRoot, artifacts.readPath, artifacts.planning)
    : { status: null };
  if (post.error) return post.error;
  const path = resolve(repoRoot, receiptPath(loaded.config, change));
  if (existsSync(path)) {
    const receipt = verifyReceipt(repoRoot, loaded.config, change);
    return receipt.ok
      ? { ...receipt, ...(post.status ? { postImplementation: post.status } : {}) }
      : receipt;
  }
  const candidate = readinessCandidate(repoRoot, change, artifacts.paths);
  if (candidate.error) return candidate.error;
  return allow("ready-for-confirmation", repoRoot, {
    change,
    artifactCommit: candidate.artifactCommit,
    artifacts: candidate.artifacts,
    taskHashNormalizationVersion: 1,
    receiptSchemaVersion: artifacts.schemaName === "spec-led-v2" ? 2 : 1,
    ...(artifacts.planning ? {
      catalogVersion: artifacts.planning.selection.catalogVersion,
      overlayVersions: artifacts.planning.selection.overlays,
    } : {}),
    ...(post.status ? { postImplementation: post.status } : {}),
  });
}

export function verifyStaged() {
  return advisoryDisabled("staged");
}

export function verifyRange() {
  return advisoryDisabled("range");
}

function options(args, allowed) {
  const values = {};
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!allowed.includes(flag)) return { error: reject("INPUT_INVALID", `Unknown option for this verifier mode: ${flag ?? "<missing>"}`) };
    if (!value || value.startsWith("--")) return { error: reject("INPUT_INVALID", `${flag} requires a value.`) };
    if (values[flag]) return { error: reject("INPUT_INVALID", `${flag} may be specified only once.`) };
    values[flag] = value;
  }
  return { values };
}

const compatibilityModes = new Map([
  ["tool", verifyTool],
  ["staged", verifyStaged],
  ["range", verifyRange],
]);

function statusCli(args) {
  const parsed = options(args, ["--cwd", "--change"]);
  return parsed.error ?? verifyStatus({ cwd: parsed.values["--cwd"] ?? process.cwd(), change: parsed.values["--change"] });
}

function cli([mode, ...rest]) {
  return compatibilityModes.get(mode)?.()
    ?? (mode === "status" ? statusCli(rest) : reject("MODE_UNSUPPORTED", "Use status, tool, staged, or range mode."));
}

const MAX_OUTPUT_BYTES = 64 * 1024;

export function serializeVerifierResult(result) {
  let emitted = result;
  let output = JSON.stringify(emitted);
  if (Buffer.byteLength(output, "utf8") > MAX_OUTPUT_BYTES) {
    emitted = reject("OUTPUT_LIMIT_EXCEEDED", `The verifier result exceeded ${MAX_OUTPUT_BYTES} bytes.`);
    output = JSON.stringify(emitted);
  }
  return { output: `${output}\n`, exitCode: emitted.ok ? 0 : 1 };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  let result;
  try {
    result = cli(process.argv.slice(2));
  } catch {
    result = reject("VERIFIER_FAILURE", "The advisory readiness report failed. Inspect repository paths, records, and Git state, then retry.");
  }
  const serialized = serializeVerifierResult(result);
  process.stdout.write(serialized.output);
  process.exitCode = serialized.exitCode;
}
