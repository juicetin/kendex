import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
	FABLE_5_1_MIN_CLAUDE_CODE_VERSION,
	resolveBundledClaudeExecutable,
	resolveClaudeExecutableForModel,
} from "../src/index.ts";
import { FABLE_5_1_MODEL_ID } from "../src/models.js";

function withTempDir(fn) {
	const dir = mkdtempSync(join(tmpdir(), "claude-bridge-fable-exec-"));
	try { return fn(dir); } finally { rmSync(dir, { recursive: true, force: true }); }
}

function versionScript(dir, name, output, executable = true) {
	const path = join(dir, name);
	writeFileSync(path, `#!/bin/sh\nprintf '%s\\n' '${output}'\n`);
	chmodSync(path, executable ? 0o755 : 0o644);
	return path;
}

function resolve(input) {
	return resolveClaudeExecutableForModel({
		modelId: FABLE_5_1_MODEL_ID,
		cwd: input.cwd,
		configured: input.configured,
		bundled: input.bundled,
		pathEnv: input.pathEnv ?? "",
	});
}

describe("Fable 5.1 Claude Code compatibility", () => {
	it("uses a compatible configured executable authoritatively", () => withTempDir((dir) => {
		const configured = versionScript(dir, "configured", "2.1.255 (Claude Code)");
		const bundled = versionScript(dir, "bundled", "2.1.259 (Claude Code)");
		assert.equal(resolve({ cwd: dir, configured, bundled, pathEnv: dir }), configured);
	}));

	it("uses the compatible bundled executable before PATH", () => withTempDir((dir) => {
		const bundled = versionScript(dir, "bundled", "2.1.255 (Claude Code)");
		versionScript(dir, "claude", "2.1.259 (Claude Code)");
		assert.equal(resolve({ cwd: dir, bundled, pathEnv: dir }), bundled);
	}));

	it("uses a compatible PATH executable when the bundle is too old", () => withTempDir((dir) => {
		const bundled = versionScript(dir, "bundled", "2.1.254 (Claude Code)");
		const pathClaude = versionScript(dir, "claude", "2.1.255 (Claude Code)");
		assert.equal(resolve({ cwd: dir, bundled, pathEnv: dir }), pathClaude);
	}));

	it("reports missing, unparseable, and too-old discovered candidates together", () => withTempDir((dir) => {
		const missing = join(dir, "missing");
		const unparseable = versionScript(dir, "claude", "Claude Code development build");
		const tooOld = versionScript(dir, "claude-code", "2.1.254 (Claude Code)");

		assert.throws(
			() => resolve({ cwd: dir, bundled: missing, pathEnv: dir }),
			(error) => {
				assert.match(error.message, new RegExp(`requires Claude Code ${FABLE_5_1_MIN_CLAUDE_CODE_VERSION} or newer`));
				assert.ok(error.message.includes(missing));
				assert.ok(error.message.includes("missing"));
				assert.ok(error.message.includes(unparseable));
				assert.ok(error.message.includes("unparseable"));
				assert.ok(error.message.includes(tooOld));
				assert.ok(error.message.includes("2.1.254"));
				return true;
			},
		);
	}));

	it("reports an unreadable configured executable without trying fallbacks", () => withTempDir((dir) => {
		const configured = versionScript(dir, "configured", "2.1.259 (Claude Code)", false);
		assert.throws(
			() => resolve({ cwd: dir, configured }),
			(error) => error.message.includes(configured) && error.message.includes("not executable"),
		);
	}));

	it("does not fall back from an incompatible configured executable", () => withTempDir((dir) => {
		const configured = versionScript(dir, "configured", "2.1.254 (Claude Code)");
		const bundled = versionScript(dir, "bundled", "2.1.259 (Claude Code)");
		assert.throws(
			() => resolve({ cwd: dir, configured, bundled, pathEnv: dir }),
			(error) => error.message.includes(configured) && !error.message.includes(bundled),
		);
	}));

	it("the installed SDK bundle meets the required version", () => {
		const bundled = resolveBundledClaudeExecutable();
		assert.ok(bundled, "the Agent SDK optional native package must be installed");
		assert.doesNotThrow(() => resolveClaudeExecutableForModel({
			modelId: FABLE_5_1_MODEL_ID,
			cwd: process.cwd(),
			bundled,
			pathEnv: "",
		}));
	});
});
