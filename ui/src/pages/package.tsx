import { useEffect, useMemo, useState } from "react";
import type { HarnessId, Scope, VersionRow } from "@/bindings";
import { ItemCustomize } from "@/components/customize/item-customize";
import { SaveBar } from "@/components/customize/save-bar";
import { MarksNote } from "@/components/marks-note";
import { PackageActions } from "@/components/package/package-actions";
import { PackageBody } from "@/components/package/package-body";
import { PackageHeader } from "@/components/package/package-header";
import { PackageTabs } from "@/components/package/package-tabs";
import { RemoveDialog } from "@/components/package/remove-dialog";
import {
  diffHarness,
  openingTab,
  openingView,
  type PackageView,
  packageVersionActions,
  useManifestBusy,
  usePackageData,
  usePackageDiff,
} from "@/components/package/use-package-data";
import { canCustomize } from "@/lib/customization";
import { useEditingPlacesSource } from "@/lib/customized-places";
import { groupItems, groupScopes, installationIn } from "@/lib/derive";
import { packageDisplayName } from "@/lib/labels";
import { PAGE_GUTTER, WIDE_CONTENT_WIDTH } from "@/lib/layout";
import { packageMarks } from "@/lib/place-marks";
import { cn } from "@/lib/utils";
import { installedRow, latestRow, versionRowLabel } from "@/lib/versions";
import { useAuditStore } from "@/stores/audit";
import { useEditorStore } from "@/stores/editor";
import { useNavStore } from "@/stores/nav";
import { useScanStore } from "@/stores/scan";
import { useUpdatesStore } from "@/stores/updates";

/** One package, full page: what it is as installed, and what you have
 *  changed about it. */
export function PackagePage() {
  const ref = useNavStore((s) => s.packageRef);
  const initialView = useNavStore((s) => s.packageView);
  const clearPackageView = useNavStore((s) => s.clearPackageView);
  const back = useNavStore((s) => s.back);
  const result = useScanStore((s) => s.result);
  const toggle = useAuditStore((s) => s.toggle);
  const editorScope = useEditorStore((s) => s.scope);
  const { dirty, saving, openScope, load, save } = useEditorStore();
  const places = useEditingPlacesSource();

  const [view, setView] = useState<PackageView>(() => openingView(initialView));
  const [tab, setTab] = useState(() => openingTab(initialView));
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [switching, setSwitching] = useState(false);
  const mutating = useManifestBusy(switching);
  useEffect(() => {
    if (initialView) clearPackageView();
  }, [initialView, clearPackageView]);

  // The manifest this package's own edits live in, loaded up front so the
  // header can say whether there are any before the tab is opened. Until
  // that lands the editor still points wherever the last package left it,
  // so the marks read this page's own scope rather than that one.
  const [pointed, setPointed] = useState(false);
  useEffect(() => {
    if (!ref) return;
    let live = true;
    setPointed(false);
    void openScope(ref.scope).then(() => {
      if (live) setPointed(true);
    });
    return () => {
      live = false;
    };
  }, [ref, openScope]);

  const group = useMemo(() => {
    if (!ref || !result) return null;
    const matching = result.items.filter(
      (item) => item.kind === ref.kind && item.name === ref.name,
    );
    return groupItems(matching)[0] ?? null;
  }, [ref, result]);

  const { meta, files, versions, load: reload } = usePackageData(ref);
  const diff = usePackageDiff(
    ref,
    view,
    diffHarness(view, group?.installations[0]?.harness ?? null),
  );
  const updatesLoaded = useUpdatesStore((s) => s.loaded);

  // The scan no longer knows this package (removed, renamed): leave the
  // way the user came.
  useEffect(() => {
    if (ref && result && !group) back();
  }, [ref, result, group, back]);

  if (!ref || !group) return null;
  // Everything the page says about the package as installed — its path,
  // its open actions, its broken-link state, which tool a comparison reads
  // — is about one installation. That is the place the page was opened at,
  // which a customized mark can name any of.
  const primary = installationIn(group, ref.scope);
  if (!primary) return null;

  const displayName = packageDisplayName(ref);
  const installed = installedRow(versions);
  const latest = latestRow(versions);
  const customizable = canCustomize(group.kind);
  const scopes = groupScopes(group);
  // Every mark in the header is about one place: the one the Customize tab
  // has open, or the one this page was opened at while it loads. The body
  // is always about the place the page was opened at — its files, its
  // versions and its edited-files notice all belong to that one.
  const { selected, forkedHere, editedRow } = packageMarks(
    places,
    group.kind,
    group.name,
    scopes,
    ref.scope,
    pointed ? editorScope : null,
  );
  // Update waits for meta (held vs following) and the update standing, and
  // is off while edits are held.
  const canUpdate =
    latest != null &&
    !latest.installed &&
    installed != null &&
    meta != null &&
    updatesLoaded &&
    editedRow == null;

  const inEveryScope = async (act: (scope: Scope) => Promise<void>) => {
    for (const scope of scopes) await act(scope);
  };

  const { switchTo, updateToLatest, follow } = packageVersionActions(
    ref,
    displayName,
    meta?.rev != null,
    setSwitching,
    reload,
  );

  const compare = (row: VersionRow) =>
    installed &&
    setView({
      mode: "diff",
      from: installed.id,
      to: row.id,
      fromLabel: versionRowLabel(installed),
      toLabel: versionRowLabel(row),
    });

  const body = (
    <PackageBody
      reference={ref}
      group={group}
      primary={primary}
      meta={meta}
      forked={forkedHere}
      editedRow={editedRow}
      versions={versions}
      files={files}
      installed={installed}
      view={view}
      setView={setView}
      diff={diff}
      busy={mutating}
      onToggle={(enable) =>
        void inEveryScope((scope) =>
          toggle(scope, group.kind, group.name, enable),
        )
      }
      onSwitchVersion={switchTo}
      onCompare={compare}
      onFollow={follow}
      onReload={reload}
    />
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PackageHeader
        kind={group.kind}
        displayName={displayName}
        description={group.description}
        place={selected}
        scopes={scopes}
        action={
          <PackageActions
            scope={primary.scope}
            kind={group.kind}
            name={group.name}
            primaryPath={primary.path}
            updateAvailable={canUpdate}
            busy={mutating}
            onUpdate={() => latest && updateToLatest(latest)}
            onPreview={() => latest && compare(latest)}
            onRemove={() => setConfirmRemove(true)}
          />
        }
      />
      <div className={cn("min-h-0 flex-1 overflow-y-auto", PAGE_GUTTER)}>
        <div className={cn("pb-8", WIDE_CONTENT_WIDTH)}>
          {/* The marks in the header and on the chips rest on the same two
              reads the Library's do, so a failure has to be sayable here
              too — this is where someone comes to act on one. */}
          <MarksNote className="mt-3" />
          <PackageTabs
            customizable={customizable}
            tab={tab}
            onTabChange={setTab}
            overview={body}
            customize={
              <ItemCustomize
                kind={group.kind}
                name={group.name}
                scopes={scopes}
                harnesses={group.harnesses as HarnessId[]}
              />
            }
          />
        </div>
      </div>
      {dirty ? (
        <SaveBar
          saving={saving}
          busy={mutating}
          onSave={() => void save()}
          onDiscard={() => void load()}
        />
      ) : null}
      <RemoveDialog
        open={confirmRemove}
        onOpenChange={setConfirmRemove}
        kind={group.kind}
        name={group.name}
        scopes={scopes}
        onGone={back}
      />
    </div>
  );
}
