import { useEffect, useMemo, useState } from "react";
import type { HarnessId, Scope, VersionRow } from "@/bindings";
import { ItemCustomize } from "@/components/customize/item-customize";
import { SaveBar } from "@/components/customize/save-bar";
import { PackageActions } from "@/components/package/package-actions";
import { PackageBody } from "@/components/package/package-body";
import { PackageHeader } from "@/components/package/package-header";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { CUSTOMIZE_TAB, OVERVIEW_TAB } from "@/lib/copy-customize";
import { canCustomize } from "@/lib/customization";
import {
  headerStanding,
  placeStandings,
  rowIn,
  standingIn,
  useEditingPlacesSource,
} from "@/lib/customized-places";
import { groupItems, groupScopes } from "@/lib/derive";
import { packageDisplayName } from "@/lib/labels";
import { PAGE_GUTTER, WIDE_CONTENT_WIDTH } from "@/lib/layout";
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
  const primary = group.installations[0];
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
  const standings = placeStandings(places, group.kind, group.name, scopes);
  const selected = headerStanding(
    standings,
    ref.scope,
    pointed ? editorScope : null,
  );
  const here = standingIn(standings, ref.scope);
  const row = rowIn(places, group.kind, group.name, ref.scope);
  const editedRow = row?.blockedByLocalEdit ? row : null;
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
      forked={here?.forked === true}
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
          {customizable ? (
            <Tabs value={tab} onValueChange={setTab}>
              <TabsList>
                <TabsTrigger value="overview">{OVERVIEW_TAB}</TabsTrigger>
                <TabsTrigger value="customize">{CUSTOMIZE_TAB}</TabsTrigger>
              </TabsList>
              <TabsContent value="overview" className="pt-6">
                {body}
              </TabsContent>
              <TabsContent value="customize" className="pt-6">
                <ItemCustomize
                  kind={group.kind}
                  name={group.name}
                  scopes={scopes}
                  harnesses={group.harnesses as HarnessId[]}
                />
              </TabsContent>
            </Tabs>
          ) : (
            body
          )}
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
