// src/core/check-lifecycle.ts
import * as z from "zod";
var Id = z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)*$/);
var Version = z.string().regex(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
var NonEmpty = z.string().min(1);
var Sha256 = z.string().regex(/^[0-9a-f]{64}$/);
var Timestamp = z.iso.datetime({ offset: true });
var ImmutableGitIdentitySchema = z.string().regex(/^(?:[0-9a-f]{40}|diff:[0-9a-f]{64})$/);
var EvidenceExpectationSchema = z.strictObject({
  kind: z.enum(["command", "review", "document", "visual", "profile", "operational"]),
  description: NonEmpty,
  freshnessHours: z.number().positive().optional()
});
var ExecutionMethodSchema = z.strictObject({
  kind: z.enum(["command", "skill", "review", "manual", "post-release"]),
  value: NonEmpty
});
var CheckDefinitionSchema = z.strictObject({
  id: Id,
  version: Version,
  family: NonEmpty,
  purpose: NonEmpty,
  applicability: z.strictObject({
    rule: NonEmpty,
    impactKeys: z.array(NonEmpty),
    changeShapes: z.array(NonEmpty),
    riskLevels: z.array(NonEmpty)
  }),
  requiredEvidence: z.array(EvidenceExpectationSchema).min(1),
  method: ExecutionMethodSchema,
  costTier: z.enum(["cheap", "standard", "expensive", "post-release"]),
  blockingPolicy: z.enum(["task-required", "tracked-post-release"]),
  waiverPolicy: z.strictObject({
    allowed: z.boolean(),
    humanApprovalRequired: z.literal(true),
    replacementEvidenceRequired: z.literal(true)
  }),
  instrumentationExpectation: z.strictObject({
    productObservability: z.boolean(),
    workflowTelemetry: z.boolean(),
    qualityAttributeScenarioIds: z.array(Id)
  }),
  owner: NonEmpty,
  supersedes: z.array(Id),
  supersededBy: Id.optional()
});
var CheckCatalogSchema = z.strictObject({
  schemaVersion: z.literal(1),
  catalogVersion: Version,
  checks: z.array(CheckDefinitionSchema).min(1)
}).superRefine(({ checks }, context) => {
  const seen = /* @__PURE__ */ new Set();
  for (const [index, check] of checks.entries()) {
    if (seen.has(check.id)) {
      context.addIssue({
        code: "custom",
        message: `Duplicate Check Definition: ${check.id}`,
        path: ["checks", index, "id"]
      });
    }
    seen.add(check.id);
    if (check.supersedes.includes(check.id) || check.supersededBy === check.id) {
      context.addIssue({
        code: "custom",
        message: `Check Definition cannot supersede itself: ${check.id}`,
        path: ["checks", index, "supersedes"]
      });
    }
  }
});
var CheckStrengtheningSchema = z.strictObject({
  checkId: Id,
  additionalImpactKeys: z.array(NonEmpty),
  additionalChangeShapes: z.array(NonEmpty),
  additionalRiskLevels: z.array(NonEmpty),
  additionalEvidence: z.array(EvidenceExpectationSchema),
  blockingPolicy: z.literal("task-required").optional(),
  waiverAllowed: z.literal(false).optional(),
  instrumentationExpectation: z.strictObject({
    productObservability: z.literal(true).optional(),
    workflowTelemetry: z.literal(true).optional(),
    qualityAttributeScenarioIds: z.array(Id)
  }).optional()
});
var RepositoryOverlaySchema = z.strictObject({
  schemaVersion: z.literal(1),
  id: Id,
  version: Version,
  baseCatalogVersion: Version,
  additions: z.array(CheckDefinitionSchema),
  strengthenings: z.array(CheckStrengtheningSchema)
});
var sortedUnique = (values) => [...new Set(values)].sort();
function applyStrengthening(check, patch) {
  check.applicability.impactKeys = sortedUnique([...check.applicability.impactKeys, ...patch.additionalImpactKeys]);
  check.applicability.changeShapes = sortedUnique([...check.applicability.changeShapes, ...patch.additionalChangeShapes]);
  check.applicability.riskLevels = sortedUnique([...check.applicability.riskLevels, ...patch.additionalRiskLevels]);
  check.requiredEvidence.push(...patch.additionalEvidence);
  if (patch.blockingPolicy) check.blockingPolicy = patch.blockingPolicy;
  if (patch.waiverAllowed === false) check.waiverPolicy.allowed = false;
  if (!patch.instrumentationExpectation) return;
  if (patch.instrumentationExpectation.productObservability) {
    check.instrumentationExpectation.productObservability = true;
  }
  if (patch.instrumentationExpectation.workflowTelemetry) {
    check.instrumentationExpectation.workflowTelemetry = true;
  }
  check.instrumentationExpectation.qualityAttributeScenarioIds = sortedUnique([
    ...check.instrumentationExpectation.qualityAttributeScenarioIds,
    ...patch.instrumentationExpectation.qualityAttributeScenarioIds
  ]);
}
function composeCatalog(catalogInput, overlayInputs) {
  const catalog = CheckCatalogSchema.parse(catalogInput);
  const overlays = overlayInputs.map((overlay) => RepositoryOverlaySchema.parse(overlay)).sort((left, right) => `${left.id}@${left.version}`.localeCompare(`${right.id}@${right.version}`));
  const overlayIds = overlays.map(({ id }) => id);
  if (new Set(overlayIds).size !== overlayIds.length) {
    throw new Error(`Duplicate repository overlay ID: ${overlayIds.find((id, index) => overlayIds.indexOf(id) !== index)}`);
  }
  const checks = new Map(catalog.checks.map((check) => [check.id, structuredClone(check)]));
  for (const overlay of overlays) {
    if (overlay.baseCatalogVersion !== catalog.catalogVersion) {
      throw new Error(`Overlay ${overlay.id}@${overlay.version} requires catalog ${overlay.baseCatalogVersion}, received ${catalog.catalogVersion}`);
    }
    for (const addition of [...overlay.additions].sort((left, right) => left.id.localeCompare(right.id))) {
      if (checks.has(addition.id)) throw new Error(`Overlay ${overlay.id} duplicates Check Definition: ${addition.id}`);
      checks.set(addition.id, structuredClone(addition));
    }
    for (const patch of [...overlay.strengthenings].sort((left, right) => left.checkId.localeCompare(right.checkId))) {
      const check = checks.get(patch.checkId);
      if (!check) throw new Error(`Overlay ${overlay.id} references unknown Check Definition: ${patch.checkId}`);
      applyStrengthening(check, patch);
    }
  }
  return CheckCatalogSchema.parse({
    ...catalog,
    checks: [...checks.values()].sort((left, right) => left.id.localeCompare(right.id))
  });
}
var ArtifactHashSchema = z.strictObject({
  path: NonEmpty,
  sha256: Sha256
});
var EvidenceReferenceSchema = z.strictObject({
  checkId: Id,
  checkVersion: Version,
  state: z.enum(["planned", "produced"]),
  uri: NonEmpty,
  sha256: Sha256.optional(),
  producer: NonEmpty,
  method: NonEmpty,
  gitIdentity: NonEmpty,
  createdAt: Timestamp,
  freshnessHours: z.number().positive()
}).superRefine((reference, context) => {
  const external = /^https?:\/\//i.test(reference.uri);
  if (reference.state === "produced" && !external && !reference.sha256) {
    context.addIssue({ code: "custom", message: "Produced local evidence requires a SHA-256 hash", path: ["sha256"] });
  }
  if (reference.state === "produced" && !ImmutableGitIdentitySchema.safeParse(reference.gitIdentity).success) {
    context.addIssue({ code: "custom", message: "Produced evidence requires a commit SHA or diff SHA-256 identity", path: ["gitIdentity"] });
  }
});
function validateEvidenceReferences(inputs, obligations, expectedGitIdentity, now) {
  const references = inputs.map((input) => EvidenceReferenceSchema.parse(input));
  const expected = new Set(obligations.map(({ checkId, checkVersion }) => `${checkId}@${checkVersion}`));
  const found = /* @__PURE__ */ new Set();
  const errors = [];
  const nowMs = Date.parse(now);
  if (!Number.isFinite(nowMs)) throw new Error(`Invalid evidence validation time: ${now}`);
  for (const reference of references) {
    const key = `${reference.checkId}@${reference.checkVersion}`;
    if (!expected.has(key)) errors.push(`Unexpected evidence binding: ${key}`);
    else found.add(key);
    if (reference.state !== "produced") errors.push(`Evidence is not produced: ${reference.uri}`);
    if (reference.gitIdentity !== expectedGitIdentity) {
      errors.push(`Evidence Git identity mismatch: ${reference.uri}`);
    }
    const createdMs = Date.parse(reference.createdAt);
    if (createdMs > nowMs) errors.push(`Evidence timestamp is in the future: ${reference.uri}`);
    if (nowMs - createdMs > reference.freshnessHours * 60 * 60 * 1e3) {
      errors.push(`Evidence is stale: ${reference.uri}`);
    }
  }
  for (const key of expected) if (!found.has(key)) errors.push(`Missing evidence: ${key}`);
  if (errors.length) throw new Error(errors.join("\n"));
  return references;
}
var HumanApprovalSchema = z.strictObject({
  name: NonEmpty,
  email: z.email(),
  approvedAt: Timestamp,
  source: z.literal("pi-interactive")
});
var CheckDispositionSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("required"),
    checkId: Id,
    checkVersion: Version,
    method: ExecutionMethodSchema,
    plannedEvidence: z.array(EvidenceReferenceSchema).min(1)
  }),
  z.strictObject({
    kind: z.literal("not-applicable"),
    checkId: Id,
    checkVersion: Version,
    reason: NonEmpty
  }),
  z.strictObject({
    kind: z.literal("human-waived"),
    checkId: Id,
    checkVersion: Version,
    approval: HumanApprovalSchema,
    rationale: NonEmpty,
    replacementEvidence: z.array(EvidenceReferenceSchema).min(1)
  })
]);
var CheckSelectionSchema = z.strictObject({
  schemaVersion: z.literal(1),
  changeName: Id,
  catalogVersion: Version,
  overlays: z.array(z.strictObject({ id: Id, version: Version })),
  selectedFrom: z.array(ArtifactHashSchema).min(1),
  changeShape: NonEmpty,
  riskLevel: NonEmpty,
  dispositions: z.array(CheckDispositionSchema)
}).superRefine(({ dispositions }, context) => {
  const seen = /* @__PURE__ */ new Set();
  for (const [index, disposition] of dispositions.entries()) {
    if (seen.has(disposition.checkId)) {
      context.addIssue({
        code: "custom",
        message: `Duplicate disposition: ${disposition.checkId}`,
        path: ["dispositions", index, "checkId"]
      });
    }
    seen.add(disposition.checkId);
  }
});
function dispositionEvidence(disposition) {
  if (disposition.kind === "required") return disposition.plannedEvidence;
  if (disposition.kind === "human-waived") return disposition.replacementEvidence;
  return [];
}
function validateCheckSelection(catalogInput, overlayInputs, selectionInput) {
  const catalog = composeCatalog(catalogInput, overlayInputs);
  const overlays = overlayInputs.map((overlay) => RepositoryOverlaySchema.parse(overlay));
  const selection = CheckSelectionSchema.parse(selectionInput);
  const errors = [];
  if (selection.catalogVersion !== catalog.catalogVersion) {
    errors.push(`Catalog version mismatch: expected ${catalog.catalogVersion}, received ${selection.catalogVersion}`);
  }
  const expectedOverlays = overlays.map(({ id, version }) => `${id}@${version}`).sort();
  const selectedOverlays = selection.overlays.map(({ id, version }) => `${id}@${version}`).sort();
  if (expectedOverlays.join("\0") !== selectedOverlays.join("\0")) {
    errors.push(`Overlay pins mismatch: expected ${expectedOverlays.join(", ") || "none"}, received ${selectedOverlays.join(", ") || "none"}`);
  }
  const definitions = new Map(catalog.checks.map((check) => [check.id, check]));
  const dispositions = new Map(selection.dispositions.map((disposition) => [disposition.checkId, disposition]));
  for (const check of catalog.checks) {
    if (!dispositions.has(check.id)) errors.push(`Missing disposition: ${check.id}`);
  }
  for (const disposition of selection.dispositions) {
    const check = definitions.get(disposition.checkId);
    if (!check) {
      errors.push(`Unknown disposition: ${disposition.checkId}`);
      continue;
    }
    if (disposition.checkVersion !== check.version) {
      errors.push(`Check version mismatch for ${check.id}: expected ${check.version}, received ${disposition.checkVersion}`);
    }
    if (disposition.kind === "required" && (disposition.method.kind !== check.method.kind || disposition.method.value !== check.method.value)) {
      errors.push(`Check method mismatch for ${check.id}: expected ${check.method.kind}:${check.method.value}`);
    }
    for (const reference of dispositionEvidence(disposition)) {
      if (reference.checkId !== check.id || reference.checkVersion !== check.version) {
        errors.push(`Evidence binding mismatch for ${check.id}: ${reference.uri}`);
      }
    }
    if (disposition.kind === "human-waived" && !check.waiverPolicy.allowed) {
      errors.push(`Waiver not permitted: ${check.id}`);
    }
  }
  if (errors.length) throw new Error(errors.join("\n"));
  return selection;
}
var PlannedEvidenceDefaults = {
  state: "planned",
  producer: "planned",
  gitIdentity: "working-tree",
  freshnessHours: 24
};
function prefillCheckSelection({
  catalog,
  overlays = [],
  changeName,
  selectedFrom,
  changeShape,
  riskLevel,
  impactKeys,
  createdAt
}) {
  const composed = composeCatalog(catalog, overlays);
  const parsedOverlays = overlays.map((overlay) => RepositoryOverlaySchema.parse(overlay));
  const affected = new Set(impactKeys);
  const matchesScope = (values, value) => values.length === 0 || values.includes("all") || values.includes(value);
  const dispositions = composed.checks.map((check) => {
    const impactApplies = check.applicability.impactKeys.length === 0 || check.applicability.impactKeys.some((key) => affected.has(key));
    const applicable = impactApplies && matchesScope(check.applicability.changeShapes, changeShape) && matchesScope(check.applicability.riskLevels, riskLevel);
    if (!applicable) {
      return {
        kind: "not-applicable",
        checkId: check.id,
        checkVersion: check.version,
        reason: `Check does not apply to shape ${changeShape}, risk ${riskLevel}, and impacts ${impactKeys.join(", ") || "none"}; required impacts are ${check.applicability.impactKeys.join(", ") || "all"}.`
      };
    }
    return {
      kind: "required",
      checkId: check.id,
      checkVersion: check.version,
      method: check.method,
      plannedEvidence: [{
        checkId: check.id,
        checkVersion: check.version,
        state: PlannedEvidenceDefaults.state,
        uri: `evidence/${check.id}.json`,
        producer: PlannedEvidenceDefaults.producer,
        method: check.method.value,
        gitIdentity: PlannedEvidenceDefaults.gitIdentity,
        createdAt,
        freshnessHours: Math.min(...check.requiredEvidence.map(({ freshnessHours }) => freshnessHours ?? PlannedEvidenceDefaults.freshnessHours))
      }]
    };
  });
  const selection = {
    schemaVersion: 1,
    changeName,
    catalogVersion: composed.catalogVersion,
    overlays: parsedOverlays.map(({ id, version }) => ({ id, version })).sort((left, right) => left.id.localeCompare(right.id)),
    selectedFrom,
    changeShape,
    riskLevel,
    dispositions
  };
  return validateCheckSelection(catalog, overlays, selection);
}
var ReviewIdentitySchema = z.strictObject({
  authorInstanceId: NonEmpty,
  reviewerInstanceId: NonEmpty,
  model: NonEmpty,
  authorModel: NonEmpty,
  differentModelAvailable: z.boolean(),
  sameModelReason: NonEmpty.optional(),
  freshContext: z.literal(true),
  role: z.literal("review-only"),
  toolPolicy: z.literal("read-only"),
  personaSwap: z.literal(false)
}).superRefine((identity, context) => {
  if (identity.authorInstanceId === identity.reviewerInstanceId) {
    context.addIssue({ code: "custom", message: "Reviewer instance must differ from author instance", path: ["reviewerInstanceId"] });
  }
  if (identity.model === identity.authorModel && identity.differentModelAvailable) {
    context.addIssue({ code: "custom", message: "A different reviewer model is available", path: ["model"] });
  }
  if (identity.model === identity.authorModel && !identity.differentModelAvailable && !identity.sameModelReason) {
    context.addIssue({ code: "custom", message: "Same-model review requires an unavailability reason", path: ["sameModelReason"] });
  }
});
var ReviewFindingSchema = z.strictObject({
  severity: z.enum(["critical", "high", "medium", "low", "info"]),
  checkId: Id.optional(),
  location: NonEmpty,
  reason: NonEmpty,
  exactCorrection: NonEmpty
});
var SelectionReviewSchema = z.strictObject({
  schemaVersion: z.literal(1),
  changeName: Id,
  reviewedArtifacts: z.array(ArtifactHashSchema).min(1),
  reviewer: ReviewIdentitySchema,
  reviewedAt: Timestamp,
  sourceReport: NonEmpty,
  sourceReportSha256: Sha256,
  verdict: z.enum(["accepted", "rejected"]),
  findings: z.array(ReviewFindingSchema)
}).superRefine((review, context) => {
  if (review.verdict === "rejected" && review.findings.length === 0) {
    context.addIssue({ code: "custom", message: "Rejected Selection Review requires exact findings", path: ["findings"] });
  }
  if (review.verdict === "accepted" && review.findings.some(({ severity }) => severity === "critical" || severity === "high")) {
    context.addIssue({ code: "custom", message: "Accepted Selection Review cannot retain critical or high findings", path: ["findings"] });
  }
});
function validateSelectionReview(selectionInput, reviewInput) {
  const selection = CheckSelectionSchema.parse(selectionInput);
  const review = SelectionReviewSchema.parse(reviewInput);
  if (review.changeName !== selection.changeName) {
    throw new Error(`Selection Review change mismatch: expected ${selection.changeName}, received ${review.changeName}`);
  }
  return review;
}
var QualityAttributeScenarioSchema = z.strictObject({
  id: Id,
  stimulus: NonEmpty,
  context: NonEmpty,
  response: NonEmpty,
  threshold: NonEmpty.regex(/\d/, "Threshold must contain a measurable numeric bound")
});
var QualityAttributeScenarioSetSchema = z.array(QualityAttributeScenarioSchema).min(1).superRefine((scenarios, context) => {
  const seen = /* @__PURE__ */ new Set();
  for (const [index, scenario] of scenarios.entries()) {
    if (seen.has(scenario.id)) {
      context.addIssue({ code: "custom", message: `Duplicate Quality Attribute Scenario: ${scenario.id}`, path: [index, "id"] });
    }
    seen.add(scenario.id);
  }
});
function validateQualityAttributeScenarios(catalogInput, scenariosInput) {
  const catalog = CheckCatalogSchema.parse(catalogInput);
  const scenarios = QualityAttributeScenarioSetSchema.parse(scenariosInput);
  const scenarioIds = new Set(scenarios.map(({ id }) => id));
  const missing = catalog.checks.flatMap(
    (check) => check.instrumentationExpectation.qualityAttributeScenarioIds.filter((id) => !scenarioIds.has(id)).map((id) => `${check.id}: ${id}`)
  );
  if (missing.length) throw new Error(`Missing Quality Attribute Scenario links:
${missing.join("\n")}`);
  return scenarios;
}
var SensitiveLogField = /^(?:pass(?:word|wd|phrase)?|secret|token|auth(?:orization)?|bearer|jwt|cookie|request|response|payload|object|credentials?|key)$/;
function logFieldParts(field) {
  return field.replace(/([a-z0-9])([A-Z])/g, "$1.$2").toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
}
var ProductObservabilitySchema = z.strictObject({
  schemaVersion: z.literal(1),
  boundaries: z.array(z.strictObject({
    name: NonEmpty,
    kind: z.enum(["system", "external", "database", "queue", "domain"]),
    spanName: NonEmpty,
    allowlistedAttributes: z.array(NonEmpty),
    correlationFields: z.array(NonEmpty),
    qualityAttributeScenarioIds: z.array(Id)
  })),
  logFieldsByLevel: z.strictObject({
    info: z.array(NonEmpty),
    debug: z.array(NonEmpty),
    trace: z.array(NonEmpty)
  }),
  profiling: z.strictObject({
    mode: z.enum(["sampled", "on-demand"]),
    scope: NonEmpty,
    durationSeconds: z.number().positive(),
    overheadPercent: z.number().nonnegative().max(100),
    sensitiveDataControls: z.array(NonEmpty).min(1),
    retention: NonEmpty
  })
}).superRefine(({ boundaries, logFieldsByLevel }, context) => {
  const boundaryNames = /* @__PURE__ */ new Set();
  for (const [index, boundary] of boundaries.entries()) {
    if (boundaryNames.has(boundary.name)) {
      context.addIssue({ code: "custom", message: `Duplicate observability boundary: ${boundary.name}`, path: ["boundaries", index, "name"] });
    }
    boundaryNames.add(boundary.name);
  }
  for (const [level, fields] of Object.entries(logFieldsByLevel)) {
    for (const [index, field] of fields.entries()) {
      if (logFieldParts(field).some((part) => SensitiveLogField.test(part))) {
        context.addIssue({ code: "custom", message: `Sensitive or unrestricted ${level} log field: ${field}`, path: ["logFieldsByLevel", level, index] });
      }
    }
  }
});
var WorkflowTelemetrySchema = z.strictObject({
  schemaVersion: z.literal(1),
  changeName: Id,
  checkId: Id,
  checkVersion: Version,
  durationMs: z.number().nonnegative(),
  cost: z.number().nonnegative().optional(),
  retries: z.number().int().nonnegative(),
  outcome: z.enum(["passed", "failed", "waived", "post-release"]),
  yield: z.enum(["found-defect", "confirmed", "no-finding", "false-positive"]),
  waiverRecorded: z.boolean()
}).superRefine((telemetry, context) => {
  if (telemetry.outcome === "waived" !== telemetry.waiverRecorded) {
    context.addIssue({ code: "custom", message: "Waiver telemetry must match the check outcome", path: ["waiverRecorded"] });
  }
});
var ThresholdBreachSchema = z.strictObject({
  changeName: Id,
  scenarioId: Id,
  threshold: NonEmpty.regex(/\d/),
  observed: NonEmpty.regex(/\d/),
  evidence: EvidenceReferenceSchema
});
function createThresholdBreachFollowUp(input) {
  const breach = ThresholdBreachSchema.parse(input);
  return {
    kind: "bead-proposal",
    title: `Triage ${breach.scenarioId} threshold breach`,
    description: `${breach.changeName} observed ${breach.observed}; threshold ${breach.threshold}; evidence ${breach.evidence.uri}.`,
    sourceMutation: false,
    automaticAction: false
  };
}
var SatisfactionDecisionSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("satisfied"),
    checkId: Id,
    checkVersion: Version,
    evidence: z.array(EvidenceReferenceSchema).min(1)
  }),
  z.strictObject({
    kind: z.literal("rejected"),
    checkId: Id,
    checkVersion: Version,
    evidence: z.array(EvidenceReferenceSchema),
    reason: NonEmpty,
    exactCorrection: NonEmpty
  }),
  z.strictObject({
    kind: z.literal("post-release"),
    checkId: Id,
    checkVersion: Version,
    owner: NonEmpty,
    environment: NonEmpty,
    trigger: NonEmpty,
    threshold: NonEmpty,
    evidenceDestination: NonEmpty,
    bead: NonEmpty,
    supportingEvidence: z.array(EvidenceReferenceSchema)
  })
]);
var SatisfactionReviewSchema = z.strictObject({
  schemaVersion: z.literal(1),
  changeName: Id,
  reviewedArtifacts: z.array(ArtifactHashSchema).min(1),
  reviewer: ReviewIdentitySchema,
  reviewedAt: Timestamp,
  sourceReport: NonEmpty,
  sourceReportSha256: Sha256,
  gitIdentity: ImmutableGitIdentitySchema,
  verdict: z.enum(["accepted", "rejected"]),
  decisions: z.array(SatisfactionDecisionSchema).min(1),
  findings: z.array(ReviewFindingSchema)
}).superRefine((review, context) => {
  const seen = /* @__PURE__ */ new Set();
  for (const [index, decision] of review.decisions.entries()) {
    if (seen.has(decision.checkId)) {
      context.addIssue({ code: "custom", message: `Duplicate Satisfaction Review decision: ${decision.checkId}`, path: ["decisions", index, "checkId"] });
    }
    seen.add(decision.checkId);
  }
  const rejected = review.decisions.some(({ kind }) => kind === "rejected");
  if (review.verdict === "accepted" && rejected) {
    context.addIssue({ code: "custom", message: "Accepted Satisfaction Review cannot contain a rejected decision", path: ["verdict"] });
  }
  if (review.verdict === "rejected" && !rejected) {
    context.addIssue({ code: "custom", message: "Rejected Satisfaction Review requires a rejected decision", path: ["decisions"] });
  }
  if (review.verdict === "accepted" && review.findings.some(({ severity }) => severity === "critical" || severity === "high")) {
    context.addIssue({ code: "custom", message: "Accepted Satisfaction Review cannot retain critical or high findings", path: ["findings"] });
  }
});
function validateSatisfactionReview(catalogInput, overlayInputs, selectionInput, reviewInput, now) {
  const catalog = composeCatalog(catalogInput, overlayInputs);
  const selection = validateCheckSelection(catalogInput, overlayInputs, selectionInput);
  const review = SatisfactionReviewSchema.parse(reviewInput);
  const errors = [];
  if (review.changeName !== selection.changeName) {
    errors.push(`Satisfaction Review change mismatch: expected ${selection.changeName}, received ${review.changeName}`);
  }
  const definitions = new Map(catalog.checks.map((check) => [check.id, check]));
  const obligations = new Map(
    selection.dispositions.filter(({ kind }) => kind !== "not-applicable").map((disposition) => [disposition.checkId, disposition.checkVersion])
  );
  const decisions = new Map(review.decisions.map((decision) => [decision.checkId, decision]));
  for (const [checkId] of obligations) if (!decisions.has(checkId)) errors.push(`Missing Satisfaction Review decision: ${checkId}`);
  for (const decision of review.decisions) {
    const expectedVersion = obligations.get(decision.checkId);
    if (!expectedVersion) {
      errors.push(`Unexpected Satisfaction Review decision: ${decision.checkId}`);
      continue;
    }
    if (decision.checkVersion !== expectedVersion) {
      errors.push(`Satisfaction Review version mismatch for ${decision.checkId}`);
      continue;
    }
    if (decision.kind === "satisfied") {
      try {
        validateEvidenceReferences(
          decision.evidence,
          [{ checkId: decision.checkId, checkVersion: decision.checkVersion }],
          review.gitIdentity,
          now
        );
      } catch (error) {
        errors.push(error instanceof Error ? error.message : String(error));
      }
    }
    if (decision.kind === "post-release") {
      const definition = definitions.get(decision.checkId);
      if (definition?.blockingPolicy !== "tracked-post-release" && definition?.method.kind !== "post-release") {
        errors.push(`Post-release decision is not permitted for ${decision.checkId}`);
      }
    }
  }
  if (errors.length) throw new Error(errors.join("\n"));
  return review;
}
var RetrospectiveFindingSchema = z.strictObject({
  id: Id,
  kind: z.enum(["check-outcome", "retry", "waiver", "review-correction", "human-intervention", "unverified-claim", "missing-guidance", "avoidable-cost"]),
  summary: NonEmpty,
  impact: z.enum(["low", "medium", "high"]),
  evidence: z.array(EvidenceReferenceSchema).min(1)
});
var ProposedAmendmentSchema = z.strictObject({
  id: Id,
  target: NonEmpty,
  owner: NonEmpty,
  evidence: z.array(EvidenceReferenceSchema).min(1),
  expectedEffect: NonEmpty,
  validationMethod: NonEmpty,
  kind: z.enum(["confirmed-repair", "new-behavior"])
});
var SkillCandidateInputSchema = z.strictObject({
  id: Id,
  problemKey: Id,
  occurrences: z.number().int().positive(),
  highImpact: z.boolean(),
  existingSkillBroken: z.boolean(),
  evidence: z.array(EvidenceReferenceSchema).min(1),
  impact: z.enum(["low", "medium", "high"]),
  proposedOwner: NonEmpty,
  trigger: NonEmpty,
  evaluationPlan: NonEmpty
});
var SkillCandidateSchema = SkillCandidateInputSchema.superRefine((candidate, context) => {
  if (candidate.existingSkillBroken) {
    context.addIssue({ code: "custom", message: "A broken existing skill must route to immediate repair", path: ["existingSkillBroken"] });
  }
  if (candidate.occurrences < 2 && !(candidate.highImpact && candidate.impact === "high")) {
    context.addIssue({ code: "custom", message: "A new Skill Candidate requires two occurrences or one high-impact failure", path: ["occurrences"] });
  }
  if (candidate.evidence.length < candidate.occurrences) {
    context.addIssue({ code: "custom", message: "Each independent occurrence requires evidence", path: ["evidence"] });
  }
});
function routeSkillCandidate(input) {
  const candidate = SkillCandidateInputSchema.parse(input);
  if (candidate.existingSkillBroken) {
    return { route: "immediate-repair", owner: candidate.proposedOwner, evidence: candidate.evidence };
  }
  if (SkillCandidateSchema.safeParse(candidate).success) {
    return { route: "skill-candidate", candidate };
  }
  return { route: "finding-only", problemKey: candidate.problemKey, evidence: candidate.evidence };
}
var EvaluationMetricSchema = z.strictObject({
  value: z.number(),
  threshold: z.number(),
  comparison: z.enum(["at-least", "at-most"]),
  unit: NonEmpty
});
var SkillEvaluationSchema = z.strictObject({
  schemaVersion: z.literal(1),
  candidateId: Id,
  evaluatorInstanceId: NonEmpty,
  evaluatedAt: Timestamp,
  triggerCases: z.strictObject({ passed: z.number().int().nonnegative(), total: z.number().int().positive() }),
  requiredBehaviorCases: z.strictObject({ passed: z.number().int().nonnegative(), total: z.number().int().positive() }),
  forbiddenBehaviorCases: z.strictObject({ passed: z.number().int().nonnegative(), total: z.number().int().positive() }),
  outcome: EvaluationMetricSchema,
  cost: EvaluationMetricSchema,
  automatedApproval: z.literal(false),
  verdict: z.enum(["passed", "failed"])
}).superRefine((evaluation, context) => {
  const complete = [evaluation.triggerCases, evaluation.requiredBehaviorCases, evaluation.forbiddenBehaviorCases].every(({ passed: passed2, total }) => passed2 === total);
  const meets = ({ value, threshold, comparison }) => comparison === "at-least" ? value >= threshold : value <= threshold;
  const passed = complete && meets(evaluation.outcome) && meets(evaluation.cost);
  if (evaluation.verdict === "passed" !== passed) {
    context.addIssue({ code: "custom", message: "Skill evaluation verdict does not match its cases and thresholds", path: ["verdict"] });
  }
});
var SkillApprovalSchema = z.strictObject({
  schemaVersion: z.literal(1),
  candidateId: Id,
  approval: HumanApprovalSchema
});
function evaluateSkillCandidate(evaluationInput, approvalInput) {
  const evaluation = SkillEvaluationSchema.parse(evaluationInput);
  if (evaluation.verdict !== "passed") {
    return { eligibleForHumanApproval: false, approved: false, installed: false };
  }
  if (approvalInput === void 0) {
    return { eligibleForHumanApproval: true, approved: false, installed: false };
  }
  const approval = SkillApprovalSchema.parse(approvalInput);
  if (approval.candidateId !== evaluation.candidateId) throw new Error("Skill approval candidate does not match evaluation");
  return { eligibleForHumanApproval: true, approved: true, installed: false, approval };
}
var RetrospectiveSchema = z.strictObject({
  schemaVersion: z.literal(1),
  changeName: Id,
  findings: z.array(RetrospectiveFindingSchema),
  proposedAmendments: z.array(ProposedAmendmentSchema),
  skillCandidates: z.array(SkillCandidateSchema),
  noFindingReason: NonEmpty.optional()
}).superRefine((retrospective, context) => {
  const hasOutput = retrospective.findings.length > 0 || retrospective.proposedAmendments.length > 0 || retrospective.skillCandidates.length > 0;
  if (!hasOutput && !retrospective.noFindingReason) {
    context.addIssue({ code: "custom", message: "Empty retrospective requires an explicit no-finding reason", path: ["noFindingReason"] });
  }
  if (hasOutput && retrospective.noFindingReason) {
    context.addIssue({ code: "custom", message: "A retrospective with findings cannot claim a no-finding outcome", path: ["noFindingReason"] });
  }
  const ids = [
    ...retrospective.findings.map(({ id }) => id),
    ...retrospective.proposedAmendments.map(({ id }) => id),
    ...retrospective.skillCandidates.map(({ id }) => id)
  ];
  if (new Set(ids).size !== ids.length) {
    context.addIssue({ code: "custom", message: "Retrospective IDs must be unique", path: [] });
  }
});
function skillFormatErrors(skillFileLines, referenceDepth) {
  return [
    ...skillFileLines >= 500 ? ["SKILL.md must stay below 500 lines"] : [],
    ...referenceDepth > 1 ? ["Skill references must stay within one level"] : []
  ];
}
var SkillIndexEntrySchema = z.strictObject({
  name: Id,
  description: NonEmpty,
  source: NonEmpty,
  owner: NonEmpty,
  umbrella: z.boolean(),
  skillFileLines: z.number().int().positive(),
  referenceDepth: z.number().int().nonnegative(),
  formatValid: z.boolean(),
  formatErrors: z.array(NonEmpty),
  contentSha256: Sha256
}).superRefine((entry, context) => {
  const expectedErrors = skillFormatErrors(entry.skillFileLines, entry.referenceDepth);
  if (entry.formatValid !== (expectedErrors.length === 0) || JSON.stringify(entry.formatErrors) !== JSON.stringify(expectedErrors)) {
    context.addIssue({ code: "custom", message: "Skill format status does not match measured limits", path: ["formatValid"] });
  }
});
var SkillIndexSchema = z.strictObject({
  schemaVersion: z.literal(1),
  generatedAt: Timestamp,
  sources: z.array(NonEmpty).min(1),
  entries: z.array(SkillIndexEntrySchema)
}).superRefine((index, context) => {
  const keys = index.entries.map(({ source }) => source);
  if (new Set(keys).size !== keys.length) {
    context.addIssue({ code: "custom", message: "Skill Index sources must be unique", path: ["entries"] });
  }
});
function buildSkillIndex(entriesInput, generatedAt, sources = ["repository", "installed"]) {
  const entries = entriesInput.map((entry) => SkillIndexEntrySchema.parse(entry)).sort((left, right) => left.source.localeCompare(right.source));
  return SkillIndexSchema.parse({ schemaVersion: 1, generatedAt, sources: sortedUnique(sources), entries });
}
function searchSkillIndex(indexInput, query) {
  const index = SkillIndexSchema.parse(indexInput);
  const terms = query.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);
  return index.entries.map((entry) => ({
    entry,
    score: terms.filter((term) => `${entry.name} ${entry.description}`.toLowerCase().includes(term)).length
  })).filter(({ score }) => score > 0).sort((left, right) => right.score - left.score || left.entry.source.localeCompare(right.entry.source)).map(({ entry }) => entry);
}
function findSkillOwner(indexInput, query) {
  const matches = searchSkillIndex(indexInput, query);
  const owner = matches.find(({ umbrella }) => umbrella) ?? null;
  if (!owner) return { status: "no-owner", matches };
  if (!owner.formatValid) return { status: "invalid-owner", owner, errors: owner.formatErrors };
  return { status: "owner", owner };
}
function assertSkillIndexFresh(indexInput, currentEntriesInput) {
  const index = SkillIndexSchema.parse(indexInput);
  const currentEntries = currentEntriesInput.map((entry) => SkillIndexEntrySchema.parse(entry));
  const expected = index.entries.map(({ source, contentSha256 }) => `${source}@${contentSha256}`).sort();
  const current = currentEntries.map(({ source, contentSha256 }) => `${source}@${contentSha256}`).sort();
  if (expected.join("\0") !== current.join("\0")) throw new Error("Skill Index is stale; regenerate it before placement");
  return index;
}
var ActivationComponents = [
  "catalog",
  "check-selection",
  "selection-review",
  "satisfaction-review",
  "retrospective",
  "spec-led-v2",
  "lifecycle-skills",
  "package-assets"
];
var ActivationCompatibilitySchema = z.strictObject({
  schemaVersion: z.literal(1),
  active: z.boolean(),
  openSpecSchemaVersion: Version,
  catalogVersion: Version,
  checkSelectionVersion: Version,
  selectionReviewVersion: Version,
  satisfactionReviewVersion: Version,
  retrospectiveVersion: Version,
  components: z.array(z.enum(ActivationComponents)).length(ActivationComponents.length)
}).superRefine(({ components }, context) => {
  if (new Set(components).size !== ActivationComponents.length || ActivationComponents.some((component) => !components.includes(component))) {
    context.addIssue({ code: "custom", message: "Activation requires the complete Option B component set", path: ["components"] });
  }
});
var ActiveCheckLifecycle = ActivationCompatibilitySchema.parse({
  schemaVersion: 1,
  active: true,
  openSpecSchemaVersion: "2.0.0",
  catalogVersion: "1.0.0",
  checkSelectionVersion: "1.0.0",
  selectionReviewVersion: "1.0.0",
  satisfactionReviewVersion: "1.0.0",
  retrospectiveVersion: "1.0.0",
  components: ActivationComponents
});
function validateActivationCompatibility(markerInput, installedInput) {
  const marker = ActivationCompatibilitySchema.parse(markerInput);
  const installed = ActivationCompatibilitySchema.parse(installedInput);
  if (!marker.active || !installed.active) throw new Error("Option B activation is not enabled");
  for (const field of [
    "openSpecSchemaVersion",
    "catalogVersion",
    "checkSelectionVersion",
    "selectionReviewVersion",
    "satisfactionReviewVersion",
    "retrospectiveVersion"
  ]) {
    if (marker[field] !== installed[field]) {
      throw new Error(`Activation version mismatch for ${field}: expected ${marker[field]}, received ${installed[field]}`);
    }
  }
  return marker;
}
var ChangeLifecycleContextSchema = z.strictObject({
  origin: z.enum(["predecessor", "catalog"]),
  state: z.enum(["in-flight", "completed", "abandoned"]),
  pinnedCatalogVersion: Version.optional(),
  reopenedByHuman: z.boolean(),
  newMutation: z.boolean(),
  hasCurrentSelection: z.boolean(),
  hasAcceptedSelectionReview: z.boolean()
});
function resolveChangeLifecycle(contextInput, activationInput) {
  const context = ChangeLifecycleContextSchema.parse(contextInput);
  if (context.origin === "catalog") {
    if (!context.pinnedCatalogVersion) throw new Error("Catalog change is missing its pinned catalog version");
    return { mode: "catalog", catalogVersion: context.pinnedCatalogVersion };
  }
  if (!context.newMutation || context.state === "in-flight") return { mode: "predecessor" };
  if (!context.reopenedByHuman) throw new Error("New mutation on a closed predecessor change requires a human reopen");
  const activation = ActivationCompatibilitySchema.parse(activationInput);
  if (!activation.active) throw new Error("Current catalog lifecycle is not active for reopened mutation");
  if (!context.hasCurrentSelection || !context.hasAcceptedSelectionReview) {
    throw new Error("Reopened mutation requires a current Check Selection and accepted Selection Review");
  }
  return { mode: "catalog", catalogVersion: activation.catalogVersion };
}
function defineCoreCheck({
  id,
  family,
  purpose,
  method,
  methodKind = "command",
  evidenceKind = "command",
  impactKeys = [],
  costTier = "standard",
  waiverAllowed = true,
  productObservability = false
}) {
  return {
    id,
    version: "1.0.0",
    family,
    purpose,
    applicability: {
      rule: impactKeys.length ? `Required when ${impactKeys.join(", ")} is affected.` : "Required for every repository mutation.",
      impactKeys,
      changeShapes: ["all"],
      riskLevels: ["all"]
    },
    requiredEvidence: [{ kind: evidenceKind, description: `${purpose} evidence.` }],
    method: { kind: methodKind, value: method },
    costTier,
    blockingPolicy: "task-required",
    waiverPolicy: {
      allowed: waiverAllowed,
      humanApprovalRequired: true,
      replacementEvidenceRequired: true
    },
    instrumentationExpectation: {
      productObservability,
      workflowTelemetry: true,
      qualityAttributeScenarioIds: []
    },
    owner: "pi-harness",
    supersedes: []
  };
}
var CoreCatalogV1 = CheckCatalogSchema.parse({
  schemaVersion: 1,
  catalogVersion: "1.0.0",
  checks: [
    defineCoreCheck({ id: "catalog-validation", family: "baseline", purpose: "Validate the pinned catalog and complete selection", method: "check-lifecycle validator", methodKind: "manual", evidenceKind: "document", waiverAllowed: false }),
    defineCoreCheck({ id: "baseline-tests", family: "baseline", purpose: "Run repository tests", method: "pnpm test" }),
    defineCoreCheck({ id: "typecheck", family: "baseline", purpose: "Run static type checking", method: "pnpm typecheck" }),
    defineCoreCheck({ id: "build", family: "baseline", purpose: "Build shipped package assets", method: "pnpm build" }),
    defineCoreCheck({ id: "openspec-validation", family: "baseline", purpose: "Validate OpenSpec contracts", method: "openspec validate --all --strict" }),
    defineCoreCheck({ id: "dependency-review", family: "dependency", purpose: "Review dependency fit and risk", method: "dependency-survey", methodKind: "skill", evidenceKind: "review", impactKeys: ["dependencies"] }),
    defineCoreCheck({ id: "test-first", family: "test", purpose: "Record red-green evidence for meaningful behavior", method: "tdd-enforcement", methodKind: "skill", evidenceKind: "review", impactKeys: ["meaningfulBehavior"], waiverAllowed: false }),
    defineCoreCheck({ id: "selection-review", family: "review", purpose: "Independently review the complete Check Selection", method: "selection-review", methodKind: "review", evidenceKind: "review", waiverAllowed: false }),
    defineCoreCheck({ id: "satisfaction-review", family: "review", purpose: "Independently judge evidence and approved intent", method: "satisfaction-review", methodKind: "review", evidenceKind: "review", costTier: "expensive", waiverAllowed: false }),
    defineCoreCheck({ id: "code-review", family: "review", purpose: "Review the final task-owned diff after deterministic checks", method: "code-review", methodKind: "skill", evidenceKind: "review", waiverAllowed: false }),
    defineCoreCheck({ id: "auth-review", family: "auth", purpose: "Review authentication and authorization boundaries", method: "auth-check", methodKind: "skill", evidenceKind: "review", impactKeys: ["securityOrAuth"] }),
    defineCoreCheck({ id: "security-scan", family: "security", purpose: "Run deterministic security scans", method: "security-scan", methodKind: "skill", evidenceKind: "review", impactKeys: ["securityOrAuth"] }),
    defineCoreCheck({ id: "api-contract", family: "api", purpose: "Validate the authoritative HTTP API contract", method: "api-spec", methodKind: "skill", evidenceKind: "review", impactKeys: ["httpApi"] }),
    defineCoreCheck({ id: "ui-evidence", family: "ui", purpose: "Capture visual and accessibility evidence", method: "ui-evidence", methodKind: "skill", evidenceKind: "visual", impactKeys: ["ui"] }),
    defineCoreCheck({ id: "quality-attribute-scenarios", family: "quality-attribute", purpose: "Validate measurable quality attribute scenarios", method: "focused scenario tests", evidenceKind: "document", impactKeys: ["meaningfulBehavior"] }),
    defineCoreCheck({ id: "product-observability", family: "observability", purpose: "Validate affected product instrumentation boundaries", method: "observability contract review", methodKind: "review", evidenceKind: "review", impactKeys: ["meaningfulBehavior"], productObservability: true }),
    defineCoreCheck({ id: "structured-logging", family: "observability", purpose: "Validate structured allowlisted log fields", method: "logging contract review", methodKind: "review", evidenceKind: "review", impactKeys: ["securityOrAuth"], productObservability: true }),
    defineCoreCheck({ id: "bounded-profiling", family: "performance", purpose: "Validate bounded profiling evidence when selected", method: "bounded profiling session", methodKind: "manual", evidenceKind: "profile", impactKeys: ["meaningfulBehavior"], costTier: "expensive", productObservability: true }),
    defineCoreCheck({ id: "workflow-telemetry", family: "observability", purpose: "Record quality-process duration, cost, retries, yield, and waivers", method: "workflow telemetry record", methodKind: "manual", evidenceKind: "document", waiverAllowed: false }),
    defineCoreCheck({ id: "skill-evaluation", family: "skill", purpose: "Evaluate changed skill triggers and behavior", method: "skill evaluation", methodKind: "review", evidenceKind: "review" }),
    defineCoreCheck({ id: "code-simplification", family: "maintainability", purpose: "Simplify task-owned code without behavior change", method: "code-simplifier", methodKind: "skill", evidenceKind: "review", impactKeys: ["meaningfulBehavior"] }),
    defineCoreCheck({ id: "maintainability-review", family: "maintainability", purpose: "Review cohesion and avoid duplicate lifecycle logic", method: "strict-maintainability-review", methodKind: "skill", evidenceKind: "review", impactKeys: ["meaningfulBehavior"] }),
    defineCoreCheck({ id: "fallow", family: "fallow", purpose: "Audit changed code for dependency and structure findings", method: "fallow-check", methodKind: "skill", evidenceKind: "review", impactKeys: ["fallow"] }),
    defineCoreCheck({ id: "mutation-testing", family: "test", purpose: "Run bounded mutation testing only after direct human opt-in", method: "mutation-testing", methodKind: "skill", evidenceKind: "review", impactKeys: ["mutationTesting"], costTier: "expensive", waiverAllowed: false }),
    defineCoreCheck({ id: "documentation-sync", family: "documentation", purpose: "Synchronize every affected active artifact", method: "documentation and diff inspection", methodKind: "review", evidenceKind: "document", waiverAllowed: false }),
    defineCoreCheck({ id: "package-verification", family: "package", purpose: "Verify shipped package contents", method: "npm pack --dry-run" }),
    defineCoreCheck({ id: "retrospective", family: "retrospective", purpose: "Produce the typed post-review retrospective", method: "retrospective", methodKind: "manual", evidenceKind: "document", waiverAllowed: false }),
    defineCoreCheck({ id: "skill-governance", family: "skill", purpose: "Validate Skill Index search, candidate threshold, evaluation, and human approval", method: "skill governance review", methodKind: "review", evidenceKind: "review", waiverAllowed: false })
  ]
});
var PortableSchemas = {
  ActivationCompatibility: ActivationCompatibilitySchema,
  CheckCatalog: CheckCatalogSchema,
  CheckSelection: CheckSelectionSchema,
  ChangeLifecycleContext: ChangeLifecycleContextSchema,
  EvidenceReference: EvidenceReferenceSchema,
  ProductObservability: ProductObservabilitySchema,
  QualityAttributeScenario: QualityAttributeScenarioSchema,
  RepositoryOverlay: RepositoryOverlaySchema,
  Retrospective: RetrospectiveSchema,
  SatisfactionReview: SatisfactionReviewSchema,
  SelectionReview: SelectionReviewSchema,
  SkillApproval: SkillApprovalSchema,
  SkillEvaluation: SkillEvaluationSchema,
  SkillIndex: SkillIndexSchema,
  ThresholdBreach: ThresholdBreachSchema,
  WorkflowTelemetry: WorkflowTelemetrySchema
};
function generatePortableSchemas() {
  return {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    $id: "https://juicetin.dev/pi-harness/check-lifecycle/v1/schema.json",
    schemaVersion: 1,
    $defs: Object.fromEntries(
      Object.entries(PortableSchemas).map(([name, schema]) => {
        const generated = z.toJSONSchema(schema, { target: "draft-2020-12", io: "input" });
        delete generated.$schema;
        return [name, generated];
      })
    )
  };
}
export {
  ActivationCompatibilitySchema,
  ActiveCheckLifecycle,
  ArtifactHashSchema,
  ChangeLifecycleContextSchema,
  CheckCatalogSchema,
  CheckDefinitionSchema,
  CheckDispositionSchema,
  CheckSelectionSchema,
  CheckStrengtheningSchema,
  CoreCatalogV1,
  EvidenceExpectationSchema,
  EvidenceReferenceSchema,
  ExecutionMethodSchema,
  HumanApprovalSchema,
  PlannedEvidenceDefaults,
  PortableSchemas,
  ProductObservabilitySchema,
  ProposedAmendmentSchema,
  QualityAttributeScenarioSchema,
  QualityAttributeScenarioSetSchema,
  RepositoryOverlaySchema,
  RetrospectiveFindingSchema,
  RetrospectiveSchema,
  ReviewFindingSchema,
  ReviewIdentitySchema,
  SatisfactionDecisionSchema,
  SatisfactionReviewSchema,
  SelectionReviewSchema,
  SkillApprovalSchema,
  SkillCandidateInputSchema,
  SkillCandidateSchema,
  SkillEvaluationSchema,
  SkillIndexEntrySchema,
  SkillIndexSchema,
  ThresholdBreachSchema,
  WorkflowTelemetrySchema,
  assertSkillIndexFresh,
  buildSkillIndex,
  composeCatalog,
  createThresholdBreachFollowUp,
  evaluateSkillCandidate,
  findSkillOwner,
  generatePortableSchemas,
  prefillCheckSelection,
  resolveChangeLifecycle,
  routeSkillCandidate,
  searchSkillIndex,
  skillFormatErrors,
  validateActivationCompatibility,
  validateCheckSelection,
  validateEvidenceReferences,
  validateQualityAttributeScenarios,
  validateSatisfactionReview,
  validateSelectionReview
};
