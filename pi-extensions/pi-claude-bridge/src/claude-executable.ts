import { type SpawnOptions, type SpawnedProcess } from "@anthropic-ai/claude-agent-sdk";
import { spawn as spawnProcess, spawnSync } from "child_process";
import { accessSync, constants as fsConstants, readFileSync, realpathSync, statSync } from "fs";
import { createRequire } from "module";
import { delimiter, dirname, join } from "path";
import { isolatedFromEnv } from "./config.js";
import { FABLE_5_1_MODEL_ID } from "./models.js";
import { DEBUG, debug } from "./debug.js";

function executableFromPath(name: string, pathEnv = process.env.PATH ?? ""): string | undefined {
	const paths = pathEnv.split(delimiter).filter(Boolean);
	for (const dir of paths) {
		const candidate = join(dir, name);
		try {
			accessSync(candidate, fsConstants.X_OK);
			return candidate;
		} catch {
			// keep searching
		}
	}
	return undefined;
}

export function resolveClaudeExecutable(configured?: string): string | undefined {
	const trimmed = configured?.trim();
	if (trimmed) return trimmed;
	// Isolated mode: never run whatever `claude` happens to be on $PATH — the
	// host app either pins an executable in config or gets the SDK's bundled
	// default, which ships inside the host bundle.
	if (isolatedFromEnv()) return undefined;
	return executableFromPath("claude") ?? executableFromPath("claude-code");
}

export const FABLE_5_1_MIN_CLAUDE_CODE_VERSION = "2.1.255";

function runtimeLibc(): "glibc" | "musl" | undefined {
	if (process.platform !== "linux") return undefined;
	const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } } | undefined;
	return report?.header?.glibcVersionRuntime ? "glibc" : "musl";
}

/** Resolve the native CLI package that the Agent SDK would select for this host. */
export function resolveBundledClaudeExecutable(): string | undefined {
	const libc = runtimeLibc();
	const suffix = libc === "musl" ? "-musl" : "";
	const packageName = `@anthropic-ai/claude-agent-sdk-${process.platform}-${process.arch}${suffix}`;
	try {
		const packageJson = createRequire(import.meta.url).resolve(`${packageName}/package.json`);
		return join(dirname(packageJson), process.platform === "win32" ? "claude.exe" : "claude");
	} catch {
		return undefined;
	}
}

interface ClaudeExecutableResolutionInput {
	modelId: string;
	cwd: string;
	configured?: string;
	bundled?: string;
	pathEnv?: string;
}

function semverTuple(value: string): [number, number, number] | undefined {
	const match = /(?:^|\s)(\d+)\.(\d+)\.(\d+)(?=\s|$)/.exec(value);
	return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : undefined;
}

function versionAtLeast(actual: [number, number, number], minimum: [number, number, number]): boolean {
	for (let i = 0; i < actual.length; i++) {
		if (actual[i] !== minimum[i]) return actual[i] > minimum[i];
	}
	return true;
}

function inspectClaudeVersion(path: string, cwd: string): { version?: string; reason?: string } {
	try {
		preflightClaudeExecutable(path, cwd);
	} catch (error) {
		const code = (error as NodeJS.ErrnoException).code;
		return { reason: code === "ENOENT" ? "missing" : code === "EACCES" ? "not executable" : `unusable (${code ?? "unknown"})` };
	}
	const result = spawnSync(path, ["--version"], { cwd, encoding: "utf8", timeout: 2_000, maxBuffer: 16_384 });
	if (result.error) {
		const code = (result.error as NodeJS.ErrnoException).code;
		return { reason: `version check failed (${code ?? result.error.name})` };
	}
	if (result.status !== 0) return { reason: `version check exited ${result.status ?? "without status"}` };
	const parsed = semverTuple(`${result.stdout}\n${result.stderr}`);
	return parsed ? { version: parsed.join(".") } : { reason: "unparseable version" };
}

/** Select a runtime compatible with Fable 5.1 before session synchronization. */
export function resolveClaudeExecutableForModel(input: ClaudeExecutableResolutionInput): string | undefined {
	if (input.modelId !== FABLE_5_1_MODEL_ID) return resolveClaudeExecutable(input.configured);
	const configured = input.configured?.trim();
	const paths = configured
		? [configured]
		: [
			input.bundled,
			...(isolatedFromEnv() ? [] : [
				executableFromPath("claude", input.pathEnv),
				executableFromPath("claude-code", input.pathEnv),
			]),
		].filter((path): path is string => Boolean(path));
	const candidates = paths.length > 0 ? [...new Set(paths)] : [input.bundled ?? "SDK bundled executable"];
	const minimum = semverTuple(FABLE_5_1_MIN_CLAUDE_CODE_VERSION)!;
	const observed: string[] = [];
	for (const path of candidates) {
		const result = inspectClaudeVersion(path, input.cwd);
		if (result.version && versionAtLeast(semverTuple(result.version)!, minimum)) return path;
		observed.push(`${path}: ${result.version ?? result.reason}`);
	}
	throw new Error(
		`Claude Fable 5.1 requires Claude Code ${FABLE_5_1_MIN_CLAUDE_CODE_VERSION} or newer before session synchronization. ` +
		`Detected candidates: ${observed.join("; ")}. Configure provider.pathToClaudeCodeExecutable with a compatible executable.`,
	);
}

export type ClaudeExecutableFileType = "elf" | "mach-o" | "pe" | "shebang-script" | "empty" | "unknown";

export interface ClaudeExecutablePreflightResult {
	path: string;
	realPath: string;
	cwd: string;
	realCwd: string;
	fileType: ClaudeExecutableFileType;
}

function errnoValue(err: unknown): string | number | undefined {
	return typeof (err as NodeJS.ErrnoException)?.errno === "number" ? (err as NodeJS.ErrnoException).errno : undefined;
}

function syscallValue(err: unknown): string | undefined {
	return typeof (err as NodeJS.ErrnoException)?.syscall === "string" ? (err as NodeJS.ErrnoException).syscall : undefined;
}

function pathValue(err: unknown): string | undefined {
	const value = (err as NodeJS.ErrnoException)?.path;
	return typeof value === "string" ? value : undefined;
}

function codeValue(err: unknown, fallback: string): string {
	const value = (err as NodeJS.ErrnoException)?.code;
	return typeof value === "string" ? value : fallback;
}

function displayValue(value: unknown): string {
	return value === undefined || value === null || value === "" ? "<none>" : String(value);
}

function makeClaudePreflightError(
	summary: string,
	details: { code: string; errno?: string | number; syscall?: string; path: string; cwd: string; fileType?: ClaudeExecutableFileType; realPath?: string; cause?: unknown },
): Error & NodeJS.ErrnoException & { cwd: string; fileType?: ClaudeExecutableFileType; realPath?: string } {
	const detail = [
		`code=${details.code}`,
		`errno=${displayValue(details.errno)}`,
		`syscall=${displayValue(details.syscall)}`,
		`path=${details.path}`,
		`cwd=${details.cwd}`,
		...(details.fileType ? [`fileType=${details.fileType}`] : []),
		...(details.realPath ? [`realPath=${details.realPath}`] : []),
	].join(" ");
	const error = new Error(`${summary} (${detail})`) as Error & NodeJS.ErrnoException & { cwd: string; fileType?: ClaudeExecutableFileType; realPath?: string };
	error.name = "ClaudeExecutablePreflightError";
	error.code = details.code;
	if (details.errno !== undefined) error.errno = typeof details.errno === "number" ? details.errno : Number(details.errno);
	if (details.syscall) error.syscall = details.syscall;
	error.path = details.path;
	error.cwd = details.cwd;
	if (details.fileType) error.fileType = details.fileType;
	if (details.realPath) error.realPath = details.realPath;
	if (details.cause !== undefined) (error as Error & { cause?: unknown }).cause = details.cause;
	return error;
}

export function classifyClaudeExecutableBytes(bytes: Uint8Array): ClaudeExecutableFileType {
	if (bytes.length === 0) return "empty";
	if (bytes.length >= 2 && bytes[0] === 0x23 && bytes[1] === 0x21) return "shebang-script";
	if (bytes.length >= 4 && bytes[0] === 0x7f && bytes[1] === 0x45 && bytes[2] === 0x4c && bytes[3] === 0x46) return "elf";
	if (bytes.length >= 2 && bytes[0] === 0x4d && bytes[1] === 0x5a) return "pe";
	if (bytes.length >= 4) {
		const magic = bytes[0] * 0x1000000 + bytes[1] * 0x10000 + bytes[2] * 0x100 + bytes[3];
		if (
			magic === 0xfeedface ||
			magic === 0xfeedfacf ||
			magic === 0xcefaedfe ||
			magic === 0xcffaedfe ||
			magic === 0xcafebabe ||
			magic === 0xbebafeca
		) return "mach-o";
	}
	return "unknown";
}

export function preflightClaudeExecutable(path: string, cwd: string): ClaudeExecutablePreflightResult {
	let realCwd: string;
	try {
		const cwdStat = statSync(cwd);
		if (!cwdStat.isDirectory()) {
			throw makeClaudePreflightError("Claude Code spawn cwd preflight failed: cwd is not a directory.", {
				code: "ENOTDIR",
				syscall: "chdir",
				path: cwd,
				cwd,
			});
		}
		accessSync(cwd, fsConstants.X_OK);
		realCwd = realpathSync(cwd);
	} catch (err) {
		if ((err as Error).name === "ClaudeExecutablePreflightError") throw err;
		throw makeClaudePreflightError("Claude Code spawn cwd preflight failed: cwd is not reachable before spawning Claude Code.", {
			code: codeValue(err, "EACCES"),
			errno: errnoValue(err),
			syscall: syscallValue(err),
			path: pathValue(err) ?? cwd,
			cwd,
			cause: err,
		});
	}

	let realPath: string;
	try {
		const stat = statSync(path);
		if (!stat.isFile()) {
			throw makeClaudePreflightError("Claude Code executable preflight failed: resolved path is not a file.", {
				code: "EACCES",
				syscall: "exec",
				path,
				cwd,
			});
		}
		accessSync(path, fsConstants.X_OK);
		realPath = realpathSync(path);
	} catch (err) {
		if ((err as Error).name === "ClaudeExecutablePreflightError") throw err;
		throw makeClaudePreflightError("Claude Code executable preflight failed: cannot access resolved executable before spawning Claude Code.", {
			code: codeValue(err, "ENOENT"),
			errno: errnoValue(err),
			syscall: syscallValue(err),
			path: pathValue(err) ?? path,
			cwd,
			cause: err,
		});
	}

	let fileType: ClaudeExecutableFileType;
	try {
		fileType = classifyClaudeExecutableBytes(readFileSync(realPath).subarray(0, 16));
	} catch (err) {
		throw makeClaudePreflightError("Claude Code executable preflight failed: cannot read executable header before spawning Claude Code.", {
			code: codeValue(err, "EACCES"),
			errno: errnoValue(err),
			syscall: syscallValue(err),
			path: pathValue(err) ?? realPath,
			cwd,
			realPath,
			cause: err,
		});
	}

	if (!["elf", "mach-o", "pe", "shebang-script"].includes(fileType)) {
		throw makeClaudePreflightError("Claude Code executable preflight failed: executable header is not an ELF, Mach-O, PE, or shebang script.", {
			code: "ENOEXEC",
			syscall: "exec",
			path,
			cwd,
			fileType,
			realPath,
		});
	}

	return { path, realPath, cwd, realCwd, fileType };
}

function envFlagEnabled(value: string | undefined): boolean {
	return value === "1" || value?.toLowerCase() === "true";
}

export function wrapClaudeSpawnErrorForSdk(err: Error, options: SpawnOptions): Error & NodeJS.ErrnoException & { cwd: string; originalCode?: string; originalMessage?: string } {
	const originalCode = codeValue(err, "SPAWN_ERROR");
	const originalMessage = err.message;
	const spawnPath = pathValue(err) ?? options.command;
	const cwd = options.cwd ?? process.cwd();
	const detail = [
		`code=${originalCode}`,
		`errno=${displayValue(errnoValue(err))}`,
		`syscall=${displayValue(syscallValue(err))}`,
		`path=${spawnPath}`,
		`cwd=${cwd}`,
		`command=${options.command}`,
	].join(" ");
	const wrapped = new Error(`Claude Code spawn failed: ${originalMessage} (${detail})`) as Error & NodeJS.ErrnoException & { cwd: string; originalCode?: string; originalMessage?: string };
	wrapped.name = "ClaudeSpawnDiagnosticError";
	// The SDK special-cases code === ENOENT and replaces the message with its
	// generic "native binary not found" text. Preserve the original code in the
	// message/originalCode while using a bridge code so the SDK surfaces context.
	wrapped.code = originalCode === "ENOENT" ? "CLAUDE_BRIDGE_SPAWN_FAILED" : originalCode;
	wrapped.originalCode = originalCode;
	wrapped.originalMessage = originalMessage;
	const errno = errnoValue(err);
	if (errno !== undefined) wrapped.errno = typeof errno === "number" ? errno : Number(errno);
	const syscall = syscallValue(err);
	if (syscall) wrapped.syscall = syscall;
	wrapped.path = spawnPath;
	wrapped.cwd = cwd;
	// Do not set `cause` here: the listener copies these structured fields back
	// onto the original Error. A cause reference to that same object would become
	// `err.cause === err`, making JSON.stringify throw on a circular structure.
	// originalMessage plus code/errno/syscall/path/cwd preserve the useful data.
	return wrapped;
}

export function spawnClaudeCodeWithDiagnostics(options: SpawnOptions): SpawnedProcess {
	const pipeStderr = DEBUG || envFlagEnabled(options.env.DEBUG_CLAUDE_AGENT_SDK);
	const child = spawnProcess(options.command, options.args, {
		cwd: options.cwd,
		env: options.env,
		signal: options.signal,
		stdio: ["pipe", "pipe", pipeStderr ? "pipe" : "ignore"],
		windowsHide: true,
	});
	if (pipeStderr) {
		child.stderr?.on("data", (data) => {
			for (const line of data.toString().split(/\r?\n/)) {
				if (line) debug(`[cli-stderr spawn] ${line}`);
			}
		});
	}
	child.prependListener("error", (err) => {
		const originalStack = err.stack;
		const wrapped = wrapClaudeSpawnErrorForSdk(err, options);
		Object.assign(err, wrapped);
		err.name = wrapped.name;
		err.message = wrapped.message;
		// Keep V8's stack from the actual Node spawn failure, not the wrapper
		// construction site. Diagnostic fields above remain enumerable and
		// JSON-serializable; stack stays the spawn-time breadcrumb for operators.
		if (originalStack) err.stack = originalStack;
	});
	return {
		stdin: child.stdin,
		stdout: child.stdout,
		get killed() { return child.killed; },
		get exitCode() { return child.exitCode; },
		kill: child.kill.bind(child),
		on: child.on.bind(child),
		once: child.once.bind(child),
		off: child.off.bind(child),
	};
}
