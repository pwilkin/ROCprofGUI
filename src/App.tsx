import { invoke as tuningInvoke } from "@tauri-apps/api/core";
import { applyWaveStats, type WaveStatsRow } from "./waveStats";
import { readWaveStats } from "./bridge";
import { buildWaitIndex } from "./waitOccurrences";
import {
  AnalysisPreferencesContext,
  type AnalysisPreferences,
} from "./analysisPreferences";
import { initialIsaState } from "./isaState";
import DockWorkspace, { defaultDockLayout } from "./components/DockWorkspace";
import {
  readTimelinePreferenceSet,
  restoreTimelinePreferenceSet,
} from "./timelinePreferences";
import {
  cuWaveLane,
  utilizationLane,
  waveLocation,
  otherSimdMarks,
} from "./waveTopology";
import {
  globalMarks,
  decoderMarks,
  dispatchMarks,
  defaultGlobalLayers,
  decoderEventGroups,
  defaultEventGroups,
} from "./globalTimeline";
import { diagnosticManifest, diagnosticBytes } from "./diagnostics";
import { diagnosticContext, saveDiagnostics } from "./bridge";
import AttSetup from "./components/AttSetup";
import { useTheme } from "./theme";
import {
  DisplayScaleContext,
  useDisplayScale,
  type StrokeMode,
} from "./displayScale";
import { useCodeFont } from "./codeFont";
import { useWaveStateImport } from "./importPreferences";
import { waveOccurrence } from "./waveViewport";
import { readWaveOverview, overviewMarks, overviewDescription, isWaveDetailBudgetError, type WaveOverview } from "./waveOverview";
import "./light-theme.css";
import { DefinitionWorkspace } from "./definitionWorkspace";
import { loadDefinitionLibrary } from "./components/DefinitionLibrary";
import {
  libraryKey,
  validateLibrary,
  type DefinitionLibrary,
} from "./definitionLibrary";
import type { ScalarDefinition } from "./scalarCounters";
import { PlotAxesContext, type PlotAxes } from "./plotAxes";
import RangeBound from "./components/RangeBound";
import RunChooser from "./components/RunChooser";
import SamplingChooser from "./components/SamplingChooser";
import {
  openCapturePath,
  openProject,
  acknowledgeOpenedProject,
  saveProject,
  exportCaptureArchive,
  attachSpm,
  type AttRun,
} from "./bridge";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { tokenLabeler } from "./tokenLabels";
import {
  Activity,
  BookOpen,
  ArrowLeft,
  ArrowRight,
  BarChart3,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Cpu,
  Download,
  FileCode2,
  Flame,
  FolderOpen,
  Gauge,
  Layers3,
  Maximize2,
  Network,
  Plus,
  Radio,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  X,
} from "lucide-react";
import {
  cancelImport,
  closeCapture,
  desktop,
  example,
  openCapture,
  readWave,
  readWaveWindow,
  analyzeHidden,
} from "./bridge";
import {
  boundedRange,
  instructionStats,
  tokenIteration,
  hiddenLatencyStats,
} from "./analysis";
import { initialTabState, type Location, type TabState } from "./session";
import type { Capture, Range, Wave, HiddenAnalysis } from "./types";
import { categories, colors, fmt } from "./types";
import { Timeline, type Mark } from "./components/Plots";
import MarkersView from "./components/MarkersView";
import IsaReference from "./components/IsaReference";
import SummaryView from "./components/SummaryView";
import PcSampling from "./components/PcSampling";
import { clockModel, utilizationToken } from "./utilization";
import { tokenSlots } from "./timelineLayout";
import { mapSource, mapSourceBatch, type SourceMappings } from "./sourceFiles";
import Instructions from "./components/Instructions";
import {
  CountersView,
  DispatchView,
  Empty,
  EvidenceView,
  FlameView,
  HardwareView,
  HotspotView,
  Metric,
  OccupancyView,
  Panel,
} from "./components/AnalysisViews";
const views = [
  { id: "reference", name: "ISA reference", icon: BookOpen },
  { id: "trace", name: "Trace workspace", icon: Activity },
  { id: "hardware", name: "Compute fabric", icon: Cpu },
  { id: "summary", name: "Activity summary", icon: BarChart3 },
  { id: "sampling", name: "PC sampling", icon: Radio },
  { id: "occupancy", name: "Occupancy", icon: Layers3 },
  { id: "dispatch", name: "Kernel dispatches", icon: Network },
  { id: "hotspots", name: "Instruction hotspots", icon: Flame },
  { id: "flame", name: "Source flamegraph", icon: BarChart3 },
  { id: "markers", name: "Marker scopes", icon: Layers3 },
  { id: "counters", name: "Counters", icon: Gauge },
  { id: "evidence", name: "Capture evidence", icon: ShieldCheck },
];
const tracePanels = [
  { id: "trace-wave", name: "Target wave" },
  { id: "trace-cu", name: "CU waves" },
  { id: "trace-util", name: "SIMD utilization" },
  { id: "trace-global", name: "Global residency" },
  { id: "trace-dispatch", name: "Dispatch timeline" },
];
export default function App() {
  const appearance = useTheme();
  const codeFont = useCodeFont();
  const waveStateImport = useWaveStateImport();
  const displayScale = useDisplayScale();
  const [captures, setCaptures] = useState<Capture[]>([]),
    [active, setActive] = useState("demo"),
    [view, setView] = useState("trace"),
    [range, setRange] = useState<Range>([0, 7000]),
    [waveId, setWaveId] = useState(""),
    [wave, setWave] = useState<Wave | null>(null),
    [waveGroup, setWaveGroup] = useState<Wave[]>([]),
    [waveGroupError, setWaveGroupError] = useState(""),
    [waveGroupLoading, setWaveGroupLoading] = useState(false),
    [waveOverview, setWaveOverview] = useState<{ capture: Capture; data: WaveOverview } | null>(null),
    [selected, setSelected] = useState(5),
    [iteration, setIteration] = useState(0),
    [mode, setMode] = useState("capture"),
    [timeline, setTimeline] = useState("wave"),
    [independentRanges, setIndependentRanges] = useState(false),
    [timelineRanges, setTimelineRanges] = useState<Record<string, Range>>({}),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [help, setHelp] = useState(false),
    [history, setHistory] = useState<Location[]>([]),
    [historyIndex, setHistoryIndex] = useState(-1);
  const [mcpSelection, setMcpSelection] = useState<Record<string, any> | null>(null);
  useEffect(() => {
    if (!desktop) return;
    let current = 0;
    const follow = async (event: Event) => {
      const generation = ++current;
      const w = (event as CustomEvent).detail;
      try {
        if (w.captureIds) {
          const next = await Promise.all((w.captureIds as string[]).map(id => capturesRef.current.find(c => c.id === id) || tuningInvoke<Capture>("mcp_capture", { id })));
          if (generation !== current) return;
          capturesRef.current = next;
          setCaptures(next);
          if (w.project) {
            tabStates.current = Object.fromEntries(next.map((c, i) => [c.id, w.project.entries[i].state]));
            const mappings = Object.fromEntries(next.map((c, i) => [c.id, w.project.entries[i].sources]));
            sourceMappingsRef.current = mappings;
            setSourceMappings(mappings);
            previousTab.current = "";
            if (w.project.definitionLibrary) setDefinitionLibrary(w.project.definitionLibrary);
          }
        }
        if (w.captureId !== undefined) setActive(w.captureId || "");
        if (w.sourceMappings && w.captureId) {
          setSourceMappings(previous => ({ ...previous, [w.captureId]: w.sourceMappings }));
        }
        setMcpSelection(w);
      } catch (e) { setError(String(e)); }
    };
    window.addEventListener("mcp-workspace", follow);
    return () => { current++; window.removeEventListener("mcp-workspace", follow); };
  }, []);
  const [sourceMappings, setSourceMappings] = useState<
    Record<string, SourceMappings>
  >({});
  const sourceMappingsRef = useRef(sourceMappings);
  sourceMappingsRef.current = sourceMappings;
  const [isaLookup, setIsaLookup] = useState("");
  const [mcpIsa, setMcpIsa] = useState<{ architecture?: string; revision: number }>();
  const [attSetup, setAttSetup] = useState<string | null>(null);
  const [samplingChoice, setSamplingChoice] = useState<string | null>(null);
  const [runChoices, setRunChoices] = useState<AttRun[] | null>(null);
  const [workspaceEpoch, setWorkspaceEpoch] = useState(0);
  const [projectStatus, setProjectStatus] = useState("");
  const [diagnosticStatus, setDiagnosticStatus] = useState("");
  const [diagnosticBusy, setDiagnosticBusy] = useState(false);
  const projectInstalled = useRef(false);
  const [hiddenResults, setHiddenResults] = useState<
    Record<string, HiddenAnalysis>
  >({});
  const hiddenRequests = useRef(new Map<Capture, AbortController>());
  const [hiddenPending, setHiddenPending] = useState(new Set<Capture>());
  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(
    () => () => {
      for (const request of hiddenRequests.current.values()) request.abort();
      hiddenRequests.current.clear();
    },
    [],
  );
  const [nonhidden, setNonhidden] = useState(false);
  const [includeIdle, setIncludeIdle] = useState(false);
  const waitIndex = useMemo(() => buildWaitIndex(wave), [wave]);
  const [initialLibrary] = useState(loadDefinitionLibrary);
  const [definitionLibrary, setDefinitionLibrary] = useState(
    initialLibrary.library,
  );
  const [definitionStorageError, setDefinitionStorageError] = useState(
    initialLibrary.error,
  );
  const [projectLibrary, setProjectLibrary] = useState(false);
  function updateDefinitionLibrary(next: DefinitionLibrary) {
    validateLibrary(next);
    setDefinitionLibrary(next);
    if (!projectLibrary) {
      try {
        localStorage.setItem(libraryKey, JSON.stringify(next));
        setDefinitionStorageError("");
      } catch (error) {
        setDefinitionStorageError(
          `Draft is only in memory; save a project or export definitions: ${String(error)}`,
        );
      }
    }
  }
  const [timelinePreferences, setTimelinePreferences] = useState(
    readTimelinePreferenceSet,
  );
  const [isaState, setIsaState] = useState(initialIsaState);
  const [dockLayout, setDockLayout] = useState(defaultDockLayout);
  const [analysisPreferences, setAnalysisPreferences] =
    useState<AnalysisPreferences>({});
  const [archiveBusy, setArchiveBusy] = useState(false);
  const [plotAxes, setPlotAxes] = useState<PlotAxes>({});
  const [histogramBins, setHistogramBins] = useState(16);
  const [cuEventGroups, setCuEventGroups] = useState(63);
  const [cuEvents, setCuEvents] = useState(true);
  const [globalLayers, setGlobalLayers] = useState(defaultGlobalLayers);
  const [plotRanges, setPlotRanges] = useState<PlotAxes>({});
  const [scalarCounters, setScalarCounters] = useState<ScalarDefinition[]>([]);
  const [highlighted, setHighlighted] = useState<number[]>([]);
  const tabStates = useRef<Record<string, TabState>>({});
  const previousTab = useRef("");
  const latestTab = useRef<TabState | null>(null);
  latestTab.current = {
    location: { waveId, instruction: selected, iteration, range, timeline },
    view,
    mode,
    timeline,
    independentRanges,
    timelineRanges,
    timelinePreferences,
    dockLayout,
    analysisPreferences,
    isaState,
    plotAxes,
    plotRanges,
    histogramBins,
    cuEventGroups,
    cuEvents,
    globalLayers,
    scalarCounters,
    nonhidden,
    includeIdle,
    history,
    historyIndex,
  };
  const capturesRef = useRef(captures);
  capturesRef.current = captures;
  const generation = useRef(0);
  const globalWaveSearch = useRef<AbortController | null>(null);
  const capture = captures.find((c) => c.id === active);
  useEffect(() => () => globalWaveSearch.current?.abort(), [capture, waveId, timeline, range]);
  const hiddenBusy = !!capture && hiddenPending.has(capture);
  useEffect(() => {
    let alive = true;
    example()
      .then((c) => {
        if (alive && !projectInstalled.current)
          setCaptures((current) =>
            current.some((item) => item.id === c.id)
              ? current
              : [c, ...current],
          );
      })
      .catch((e) => setError(String(e)));
    return () => {
      alive = false;
    };
  }, []);
  useEffect(() => {
    if (!capture) return;
    if (
      previousTab.current &&
      latestTab.current &&
      capturesRef.current.some((item) => item.id === previousTab.current)
    )
      tabStates.current[previousTab.current] = latestTab.current;
    const firstCapture = !previousTab.current;
    previousTab.current = active;
    const saved = tabStates.current[active];
    const location = saved?.location || {
      waveId: capture.waves[0]?.id || "",
      instruction: capture.instructions[1]?.index || 0,
      iteration: 0,
      range: [capture.start, capture.end] as Range,
    };
    setWave(null);
    setWaveId(location.waveId);
    setRange(location.range);
    setSelected(location.instruction);
    setIteration(location.iteration);
    setHistory(saved?.history || [location]);
    setHistoryIndex(saved?.historyIndex ?? 0);
    setView(
      saved?.view ||
        (firstCapture ? latestTab.current?.view || "trace" : "trace"),
    );
    setMode(saved?.mode || "capture");
    setTimeline(saved?.timeline || "wave");
    setIndependentRanges(saved?.independentRanges || false);
    setTimelineRanges(saved?.timelineRanges || {});
    setTimelinePreferences(
      restoreTimelinePreferenceSet(saved?.timelinePreferences),
    );
    setDockLayout(saved?.dockLayout || defaultDockLayout());
    setAnalysisPreferences(saved?.analysisPreferences || {});
    setIsaState(saved?.isaState || initialIsaState());
    setPlotAxes(saved?.plotAxes || {});
    setPlotRanges(saved?.plotRanges || {});
    setHistogramBins(saved?.histogramBins || 16);
    setCuEventGroups(saved?.cuEventGroups ?? 63);
    setCuEvents(saved?.cuEvents ?? true);
    setGlobalLayers({ ...defaultGlobalLayers, ...saved?.globalLayers });
    setScalarCounters(saved?.scalarCounters || []);
    setNonhidden(saved?.nonhidden || false);
    setIncludeIdle(saved?.includeIdle || false);
    setHighlighted([]);
  }, [active, capture?.name, workspaceEpoch]);
  useEffect(() => {
    const w = mcpSelection;
    if (!w) return;
        if (w.isaQuery !== undefined || w.isaArchitecture !== undefined) {
          setIsaLookup(w.isaQuery || "");
          setMcpIsa(previous => ({architecture:w.isaArchitecture,revision:(previous?.revision||0)+1}));
        }
        if (w.view) setView(w.view);
        if (w.range) setRange(w.range);
        if (w.waveId !== undefined) setWaveId(w.waveId);
        if (w.instruction !== undefined) setSelected(w.instruction);
        if (w.timeline) setTimeline(w.timeline);
        if (w.dispatchId && w.captureId) {
          const capture = capturesRef.current.find(c => c.id === w.captureId);
          const dispatch = capture?.dispatches.find(d => d.id === w.dispatchId);
          if (dispatch) setRange([dispatch.start, dispatch.end]);
        }
  }, [mcpSelection, active]);

  useEffect(() => {
    const n = ++generation.current;
    const controller = new AbortController();
    setWave(null);
    if (capture && waveId && capture.waves.some((w) => w.id === waveId))
      readWave(capture, waveId, controller.signal)
        .then((w) => {
          if (!controller.signal.aborted && generation.current === n) setWave(w);
        })
        .catch((e) => {
          if (!controller.signal.aborted && generation.current === n) setError(String(e));
        });
    return () => controller.abort();
  }, [active, waveId, capture]);
  const [separateLds, setSeparateLds] = useState(false);
  const [visibleStats, setVisibleStats] = useState<{capture: Capture; se: number; range: Range; rows: WaveStatsRow[]} | null>(null);
  const [visibleStatsError, setVisibleStatsError] = useState("");
  const targetSe = capture?.waves.find((w) => w.id === waveId)?.se ?? 0;
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    setVisibleStats(null);
    setVisibleStatsError("");
    if (!capture || !mode.startsWith("all") || hiddenResults[capture.id]) return;
    const timer = setTimeout(() => {
      readWaveStats(capture, targetSe, range[0], range[1], controller.signal).then(rows => {
        if (current) setVisibleStats({capture, se: targetSe, range, rows});
      }).catch(error => { if (current) setVisibleStatsError(String(error)); });
    }, 150);
    return () => { current = false; controller.abort(); clearTimeout(timer); };
  }, [capture, targetSe, range, mode, hiddenResults]);
  const currentStats = visibleStats && visibleStats.capture === capture && visibleStats.se === targetSe && visibleStats.range[0] === range[0] && visibleStats.range[1] === range[1] ? visibleStats.rows : null;
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    setWaveGroup([]);
    setWaveGroupError("");
    setWaveGroupLoading(false);
    setWaveOverview(null);
    if (
      !capture ||
      !["cu", "util"].includes(timeline)
    )
      return;
    setWaveGroupLoading(true);
    const timer = setTimeout(
      () =>
        readWaveWindow(capture, targetSe, range[0], range[1], controller.signal)
          .then((w) => {
            if (current) setWaveGroup(w);
          })
          .catch(async (e) => {
            if (!current) return;
            if (["cu", "util"].includes(timeline) && isWaveDetailBudgetError(e)) {
              try {
                const data = await readWaveOverview(capture, targetSe, range[0], range[1], timeline === "util", separateLds, controller.signal);
                if (current) setWaveOverview({ capture, data });
              } catch (error) {
                if (current) setWaveGroupError(String(error));
              }
            } else setWaveGroupError(String(e));
          })
          .finally(() => {
            if (current) setWaveGroupLoading(false);
          }),
      150,
    );
    return () => {
      current = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [capture, targetSe, range, timeline, mode, separateLds]);
  const [pinnedWaves, setPinnedWaves] = useState<{
    capture: Capture | null;
    groups: Record<string, Wave[]>;
    errors: Record<string, string>;
    overviews: Record<string, WaveOverview>;
    pending: Record<string, boolean>;
  }>({ capture: null, groups: {}, errors: {}, overviews: {}, pending: {} });
  useEffect(() => {
    let current = true;
    const controller = new AbortController();
    setPinnedWaves({ capture: capture || null, groups: {}, errors: {}, overviews: {}, pending: {} });
    if (!capture) return;
    const ids = dockLayout.pinned
      .filter((id) => id === "trace-cu" || id === "trace-util")
      .map((id) => id.slice(6))
      .filter((id) => id !== timeline);
    setPinnedWaves((previous) => ({ ...previous, pending: Object.fromEntries(ids.map((id) => [id, true])) }));
    const timer = setTimeout(() => {
      for (const id of ids) {
        const window = independentRanges ? timelineRanges[id] || range : range;
        readWaveWindow(capture, targetSe, window[0], window[1], controller.signal)
          .then((waves) => {
            if (current)
              setPinnedWaves((previous) => ({
                ...previous,
                groups: { ...previous.groups, [id]: waves },
              }));
          })
          .catch(async (error) => {
            if (!current) return;
            if (["cu", "util"].includes(id) && isWaveDetailBudgetError(error)) {
              try {
                const overview = await readWaveOverview(capture, targetSe, window[0], window[1], id === "util", separateLds, controller.signal);
                if (current) setPinnedWaves((previous) => ({ ...previous, overviews: { ...previous.overviews, [id]: overview } }));
                return;
              } catch (failure) { error = failure; }
            }
            if (current)
              setPinnedWaves((previous) => ({
                ...previous,
                errors: { ...previous.errors, [id]: String(error) },
              }));
          })
          .finally(() => {
            if (current) setPinnedWaves((previous) => ({ ...previous, pending: { ...previous.pending, [id]: false } }));
          });
      }
    }, 150);
    return () => {
      current = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [
    capture,
    targetSe,
    timeline,
    range,
    timelineRanges,
    independentRanges,
    dockLayout.pinned,
    separateLds,
  ]);
  async function runHidden(current: Capture) {
    if (hiddenRequests.current.has(current)) return;
    const request = new AbortController();
    hiddenRequests.current.set(current, request);
    setHiddenPending((previous) => new Set(previous).add(current));
    const valid = () =>
      !request.signal.aborted && hiddenRequests.current.get(current) === request &&
      capturesRef.current.includes(current);
    try {
      const result = await analyzeHidden(current, request.signal);
      if (valid())
        setHiddenResults((previous) => ({ ...previous, [current.id]: result }));
    } catch (error) {
      if (valid() && activeRef.current === current.id) setError(String(error));
    } finally {
      if (hiddenRequests.current.get(current) === request) {
        hiddenRequests.current.delete(current);
        setHiddenPending((previous) => {
          const next = new Set(previous);
          next.delete(current);
          return next;
        });
      }
    }
  }
  useEffect(() => {
    if (
      capture?.waves.length &&
      !hiddenResults[capture.id] &&
      (Number(capture.metadata.gfxip) >= 10 ||
        /^gfx1[0-9]/.test(capture.architecture))
    )
      void runHidden(capture);
  }, [capture]);
  async function load(directory: boolean) {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      const c = await openCapture(directory);
      if (c && "attSetup" in c) {
        setAttSetup(c.attSetup);
        return;
      }
      if (c && "samplingChoice" in c) {
        setSamplingChoice(c.samplingChoice);
        return;
      }
      if (c && "runs" in c) {
        setRunChoices(c.runs);
        return;
      }
      if (c) {
        setCaptures((cs) => [...cs, c]);
        setActive(c.id);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function saveWorkspace() {
    if (!latestTab.current) return;
    setBusy(true);
    setError("");
    setProjectStatus("");
    try {
      const entries = captures.map((c) => ({
        captureId: c.id,
        state:
          c.id === active
            ? latestTab.current!
            : tabStates.current[c.id] || initialTabState(c),
        sources: sourceMappingsRef.current[c.id] || {},
      }));
      if (entries.some((e) => !e.state))
        throw Error("Capture state is still initializing; try saving again");
      const saved = await saveProject(
        entries,
        captures.findIndex((c) => c.id === active),
        definitionLibrary,
      );
      setProjectStatus(
        saved
          ? "Project saved. Capture data is referenced in place; source replacements are embedded."
          : "Project save cancelled",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function archiveCapture() {
    if (!capture || busy) return;
    setBusy(true);
    setArchiveBusy(true);
    setError("");
    try {
      const saved = await exportCaptureArchive(capture.id);
      setProjectStatus(
        saved
          ? "Portable capture saved with decoded data and captured sources. Save a project separately for workspace preferences and source replacements."
          : "Capture archive export cancelled",
      );
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
      setArchiveBusy(false);
    }
  }
  async function attachSpmCapture() {
    if (!capture || busy) return;
    setBusy(true);
    setError("");
    try {
      const combined = await attachSpm(capture.id, targetSe);
      if (combined) {
        setCaptures((previous) => [...previous, combined]);
        setActive(combined.id);
        setProjectStatus(
          "Combined trace + SPM archive saved and opened. Save project to retain this tab.",
        );
      } else setProjectStatus("SPM attachment cancelled.");
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }
  async function reopenWorkspace() {
    setBusy(true);
    setError("");
    setProjectStatus("");
    try {
      const opened = await openProject();
      if (!opened) {
        setProjectStatus("Project open cancelled");
        return;
      }
      if (opened.project.definitionLibrary) {
        setDefinitionLibrary(opened.project.definitionLibrary);
        setProjectLibrary(true);
        setDefinitionStorageError("");
      }
      projectInstalled.current = true;
      generation.current++;
      tabStates.current = Object.fromEntries(
        opened.captures.map((c, n) => [c.id, opened.project.entries[n].state]),
      );
      previousTab.current = "";
      const mappings = Object.fromEntries(
        opened.captures.map((c, n) => [
          c.id,
          opened.project.entries[n].sources,
        ]),
      );
      sourceMappingsRef.current = mappings;
      setSourceMappings(mappings);
      capturesRef.current = opened.captures;
      setCaptures(opened.captures);
      setActive(opened.captures[opened.project.active].id);
      setWave(null);
      setWaveGroup([]);
      for (const request of hiddenRequests.current.values()) request.abort();
      hiddenRequests.current.clear();
      setHiddenPending(new Set());
      setHiddenResults({});
      setWorkspaceEpoch((n) => n + 1);
      setProjectStatus(
        opened.project.entries.some((entry) => entry.input !== null) &&
          opened.project.entries.every(
            (entry) => entry.input === null || entry.fingerprint,
          )
          ? "Project capture inputs match their saved fingerprints. Source replacements remain revision-unverified."
          : "Project reopened from live input references. Verify input and source revisions before comparison.",
      );
      await acknowledgeOpenedProject(opened);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function loadRun(run: Pick<AttRun, "path"> & { samplingMethod?: string }) {
    setRunChoices(null);
    setSamplingChoice(null);
    setBusy(true);
    setError("");
    try {
      const capture = await openCapturePath(run.path, run.samplingMethod);
      setCaptures((previous) => [...previous, capture]);
      setActive(capture.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function close(id: string) {
    setError("");
    for (const [current, request] of hiddenRequests.current) if (current.id === id) request.abort();
    try {
      await closeCapture(id);
    } catch (error) {
      setError(`Could not close capture: ${String(error)}`);
      return;
    }
    delete tabStates.current[id];
    if (previousTab.current === id) previousTab.current = "";
    for (const current of hiddenRequests.current.keys())
      if (current.id === id) { hiddenRequests.current.get(current)?.abort(); hiddenRequests.current.delete(current); }
    setHiddenPending(
      (previous) => new Set([...previous].filter((c) => c.id !== id)),
    );
    setHiddenResults((previous) => {
      const next = { ...previous };
      delete next[id];
      return next;
    });
    capturesRef.current = capturesRef.current.filter(
      (capture) => capture.id !== id,
    );
    setSourceMappings((old) => {
      const next = { ...old };
      delete next[id];
      return next;
    });
    setCaptures((cs) => cs.filter((c) => c.id !== id));
    setActive((current) => current === id ? capturesRef.current[0]?.id || "" : current);
  }
  function changeTimeline(id: string, explicitRange?: Range) {
    if (id === timeline && !explicitRange) return;
    if (independentRanges) {
      setTimelineRanges((previous) => ({ ...previous, [timeline]: range }));
      setRange(explicitRange || timelineRanges[id] || range);
    } else if (explicitRange) setRange(explicitRange);
    setTimeline(id);
  }
  function changeRangeLink(independent: boolean) {
    setIndependentRanges(independent);
    setTimelineRanges({});
  }
  function select(
    id: number,
    targetWave = waveId,
    occurrence = 0,
    targetTimeline = timeline,
    requestedRange?: Range,
  ) {
    setSelected(id);
    setWaveId(targetWave);
    setIteration(occurrence);
    setHighlighted([]);
    const selectedWave =
      targetWave === wave?.id
        ? wave
        : waveGroup.find((w) => w.id === targetWave);
    const token =
      selectedWave === wave
        ? waitIndex.tokenAt(id, occurrence)
        : selectedWave ? waveOccurrence(selectedWave, id, occurrence) : undefined;
    let nextRange = requestedRange || range;
    if (
      !requestedRange &&
      token &&
      capture &&
      (token.start < range[0] || token.start >= range[1])
    ) {
      const span = range[1] - range[0];
      nextRange = boundedRange(
        [token.start - span * 0.2, token.start + span * 0.8],
        [capture.start, capture.end],
      );
    }
    if (targetTimeline !== timeline) changeTimeline(targetTimeline, nextRange);
    else if (nextRange !== range) setRange(nextRange);
    const location = {
      waveId: targetWave,
      instruction: id,
      iteration: occurrence,
      range: nextRange,
      timeline: targetTimeline,
    };
    const next = history.slice(0, historyIndex + 1);
    if (next.length)
      next[next.length - 1] = {
        waveId,
        instruction: selected,
        iteration,
        range,
        timeline,
      };
    next.push(location);
    setHistory(next);
    setHistoryIndex(next.length - 1);
  }
  function restoreHistory(index: number) {
    const location = history[index];
    if (!location) return;
    setWaveId(location.waveId);
    setSelected(location.instruction);
    setIteration(location.iteration);
    changeTimeline(location.timeline || timeline, location.range);
    setHistoryIndex(index);
    setHighlighted([]);
  }
  function jump(id: number) {
    select(id);
    setView("trace");
  }
  function jumpGroup(ids: number[]) {
    if (!ids.length) return;
    jump(ids[0]);
    setHighlighted(ids);
  }
  function chooseIteration(n: number) {
    select(selected, waveId, n);
  }
  async function openGlobalWave(mark: Mark) {
    globalWaveSearch.current?.abort();
    if (!capture) return;
    const currentGeneration = generation.current;
    const interval = capture.intervals[mark.recordIndex ?? Number(mark.key)];
    if (!interval) return;
    const controller = new AbortController();
    globalWaveSearch.current = controller;
    const current = () => !controller.signal.aborted && generation.current === currentGeneration;
    try {
      const candidates = capture.waves.filter(
        (w) =>
          w.se === interval.se &&
          w.simd === interval.simd &&
          w.slot === interval.slot &&
          w.start <= interval.end &&
          w.end >= interval.start,
      );
      for (const index of candidates) {
        try {
          if (!current()) return;
          const candidate = await readWave(capture, index.id, controller.signal);
          if (!current()) return;
          if (candidate.cu !== interval.cu) continue;
          const token =
            candidate.tokens.find((t) => t.start >= interval.start) ||
            candidate.tokens[0];
          setWave(candidate);
          select(token?.instruction ?? selected, index.id, 0, "wave", [
            candidate.start,
            candidate.end,
          ]);
          return;
        } catch (e) {
          if (current()) setError(String(e));
          return;
        }
      }
      if (!current()) return;
      setError(
        `No detailed wave tokens were captured for SE ${interval.se}, CU ${interval.cu}, SIMD ${interval.simd}, slot ${interval.slot}. Global residency does not imply detailed trace coverage.`,
      );
    } finally {
      if (globalWaveSearch.current === controller) globalWaveSearch.current = null;
    }
  }

  const [globalSelection, setGlobalSelection] = useState<{
    capture: Capture;
    mark: Mark;
  } | null>(null);
  useEffect(() => {
    setGlobalSelection((selection) =>
      selection && !captures.includes(selection.capture) ? null : selection,
    );
  }, [captures]);
  const utilizationModel = capture ? clockModel(capture) : null;
  const buildMarks = useCallback(
    (timeline: string, waveGroup: Wave[]): Mark[] => {
      if (!capture) return [];
      if (timeline === "util" && !utilizationModel) return [];
      if (timeline === "global")
        return globalMarks(
          capture,
          globalLayers,
          globalSelection?.capture === capture
            ? globalSelection.mark.key
            : undefined,
        );
      if (timeline === "dispatch") return dispatchMarks(capture);
      const isa =
        timeline === "util"
          ? new Map(capture.instructions.map((i) => [i.index, i.text]))
          : null;
      const group =
        timeline === "cu" || timeline === "util"
          ? waveGroup
          : wave
            ? [wave]
            : [];
      const waveMarks: Mark[] = [];
      for (const current of group) {
        const index = capture.waves.find((w) => w.id === current.id);
        const labelForToken = tokenLabeler(waveLocation(current, index));
        const cuLane = timeline === "cu" ? cuWaveLane(current, index) : "";
        const laneCache = new Map<string | number, string>();
        const slots = current.tokenIdentities ? [] : tokenSlots(current.tokens);
        const occurrences = new Map<number, number>();
        for (let n = 0; n < current.tokens.length; n++) {
          const t = current.tokens[n];
          const identity = current.tokenIdentities?.[n];
          const occurrence = identity?.occurrence ?? (occurrences.get(t.instruction) || 0);
          occurrences.set(t.instruction, occurrence + 1);
          const projected =
            timeline === "util" && utilizationModel
              ? utilizationToken(
                  t,
                  utilizationModel,
                  isa?.get(t.instruction) || "",
                  separateLds,
                )
              : null;
          if (timeline === "util" && !projected) continue;
          const laneKey = timeline === "util" ? `${!!projected?.shared}:${projected?.pipe || "Unknown"}` : t.category;
          let lane = timeline === "cu" ? cuLane : laneCache.get(laneKey);
          if (lane === undefined) {
            lane = timeline === "util"
              ? utilizationLane(current, index, !!projected?.shared, projected?.pipe || "Unknown")
              : `SIMD ${index?.simd} · ${categories[t.category] || "OTHER"}`;
            laneCache.set(laneKey, lane);
          }
          waveMarks.push({
              start: projected?.start ?? t.start,
              end: projected?.end ?? t.start + t.duration,
              lane,
              color: colors[t.category] || colors[0],
              label: labelForToken(t),
              key: `${current.id}:${identity?.index ?? n}`,
              tokenCategory: t.category,
              mipGroup: timeline === "util" ? lane : current.id,
              shaderEngine: timeline === "cu" ? index?.se : undefined,
              waveId: current.id,
              tokenIndex: identity?.index ?? n,
              tokenOccurrence: occurrence,
              instruction: t.instruction,
              displaySlot: timeline === "util" ? undefined : (identity?.displaySlot ?? slots[n]),
              selected:
                current.id === waveId &&
                t.instruction === selected &&
                occurrence === iteration,
          });
        }
      }
      if (timeline === "util") {
        const activity = otherSimdMarks(
          capture,
          targetSe,
          capture.waves.find((w) => w.id === waveId)?.simd ?? 0,
        );
        for (const mark of activity)
          mark.selected =
            globalSelection?.capture === capture &&
            globalSelection.mark.key === mark.key;
        for (const mark of activity) waveMarks.push(mark);
        return waveMarks;
      }
      if (timeline !== "cu") return waveMarks;
      const events = decoderMarks(
        capture,
        {
          ...defaultGlobalLayers,
          events: cuEvents,
          eventGroups: cuEventGroups,
        },
        "cu",
        targetSe,
      );
      for (const event of events)
        event.selected =
          globalSelection?.capture === capture &&
          globalSelection.mark.key === event.key;
      for (const event of events) waveMarks.push(event);
      return waveMarks;
    },
    [
      cuEvents,
      cuEventGroups,
      targetSe,
      capture,
      wave,
      waveId,
      selected,
      iteration,
      separateLds,
      globalLayers,
      globalSelection,
    ],
  );
  const currentOverview = ["cu", "util"].includes(timeline) && waveOverview && waveOverview.capture === capture && !!waveOverview.data.utilization === (timeline === "util") &&
      waveOverview.data.range[0] === range[0] && waveOverview.data.range[1] === range[1]
      ? waveOverview.data
      : undefined;
  const marksWaveGroup = ["cu", "util"].includes(timeline) ? waveGroup : null;
  const marks = useMemo(
    () => currentOverview
      ? [...overviewMarks(currentOverview), ...buildMarks(timeline, [])]
      : buildMarks(timeline, marksWaveGroup ?? []),
    [buildMarks, timeline, marksWaveGroup, currentOverview],
  );
  const pinnedMarks = useMemo(
    () =>
      Object.fromEntries(
        dockLayout.pinned
          .filter((id) => id.startsWith("trace-"))
          .map((id) => {
            const target = id.slice(6);
            const group =
              target === timeline
                ? waveGroup
                : pinnedWaves.capture === capture
                  ? pinnedWaves.groups[target] || []
                  : [];
            const overview = pinnedWaves.capture === capture ? pinnedWaves.overviews[target] : undefined;
            const window = independentRanges ? timelineRanges[target] || range : range;
            const overviewMatches = overview && overview.range[0] === window[0] && overview.range[1] === window[1];
            return [target, target === timeline ? marks : overviewMatches
              ? [...overviewMarks(overview), ...buildMarks(target, [])]
              : buildMarks(target, group)];
          }),
      ),
    [buildMarks, timeline, waveGroup, pinnedWaves, capture, dockLayout.pinned, marks, independentRanges, timelineRanges, range],
  );
  const analysisWave = wave;
  const instructions = useMemo(
    () =>
      capture && hiddenResults[capture.id]
        ? hiddenLatencyStats(
            capture,
            hiddenResults[capture.id],
            mode,
            waveId,
            targetSe,
            range,
            nonhidden,
            includeIdle,
            iteration,
          )
        : mode.startsWith("all")
          ? applyWaveStats(capture?.instructions || [], currentStats, mode === "all-mean")
          : instructionStats(
            capture?.instructions || [],
            analysisWave,
            range,
            mode === "all-mean" ? "mean" : mode === "all" ? "wave" : mode,
            iteration,
          ),
    [
      capture,
      analysisWave,
      currentStats,
      range,
      mode,
      hiddenResults,
      nonhidden,
      includeIdle,
      iteration,
      waveId,
      targetSe,
    ],
  );
  const sourceIncludeHidden =
    analysisPreferences["source.includeHidden"] !== false;
  const sourceInstructions = useMemo(() => {
    if (
      !capture ||
      !hiddenResults[capture.id] ||
      sourceIncludeHidden === !nonhidden
    )
      return instructions;
    return hiddenLatencyStats(
      capture,
      hiddenResults[capture.id],
      mode,
      waveId,
      targetSe,
      range,
      !sourceIncludeHidden,
      includeIdle,
      iteration,
    );
  }, [
    capture,
    hiddenResults,
    sourceIncludeHidden,
    nonhidden,
    instructions,
    mode,
    waveId,
    targetSe,
    range,
    includeIdle,
    iteration,
  ]);
  const summedInstructions = useMemo(
    () =>
      capture && hiddenResults[capture.id]
        ? hiddenLatencyStats(
            capture,
            hiddenResults[capture.id],
            mode === "capture-mean"
              ? "capture"
              : mode === "all-mean"
                ? "all"
                : ["mean", "max", "iteration"].includes(mode)
                  ? "wave"
                  : mode,
            waveId,
            targetSe,
            range,
            nonhidden,
            false,
          )
        : mode.startsWith("all")
          ? applyWaveStats(capture?.instructions || [], currentStats)
          : instructionStats(
            capture?.instructions || [],
            analysisWave,
            range,
            mode.startsWith("capture") ? "capture" : "wave",
          ),
    [
      capture,
      analysisWave,
      currentStats,
      range,
      mode,
      hiddenResults,
      nonhidden,
      waveId,
      targetSe,
    ],
  );
  const windex = capture?.waves.find((w) => w.id === waveId);
  function filterWave(field: "se" | "simd" | "slot" | "wid", value: number) {
    if (!capture) return;
    const candidates = capture.waves.filter((w) => w[field] === value);
    const next =
      candidates.find(
        (w) =>
          (field === "se" || w.se === windex?.se) &&
          (field === "se" || field === "simd" || w.simd === windex?.simd) &&
          (field !== "wid" || w.slot === windex?.slot),
      ) || candidates[0];
    if (next) {
      select(selected, next.id, 0);
    }
  }
  async function exportDiagnosticManifest() {
    if (!capture || !latestTab.current || diagnosticBusy) return;
    const snapshot = {
      capture,
      workspace: structuredClone(latestTab.current),
      sources: structuredClone(sourceMappingsRef.current[capture.id] || {}),
      definitions: structuredClone(definitionLibrary),
      hidden: hiddenResults[capture.id],
      hiddenPending: hiddenPending.has(capture),
      wave,
    };
    setDiagnosticBusy(true);
    setDiagnosticStatus("Generating diagnostic manifest…");
    try {
      const runtime = await diagnosticContext(capture.id);
      const report = await diagnosticManifest({ ...snapshot, runtime });
      const saved = await saveDiagnostics(diagnosticBytes(report));
      setDiagnosticStatus(
        saved
          ? `Diagnostic manifest saved for ${capture.name}`
          : "Diagnostic export cancelled",
      );
    } catch (error) {
      setDiagnosticStatus(`Diagnostic export failed: ${String(error)}`);
    } finally {
      setDiagnosticBusy(false);
    }
  }
  function exportEvidence() {
    if (!capture) return;
    const blob = new Blob(
      [
        JSON.stringify(
          {
            schemaVersion: 1,
            capture: {
              name: capture.name,
              format: capture.format,
              architecture: capture.architecture,
              unit: capture.unit,
              origin: capture.origin,
              metadata: capture.metadata,
              warnings: capture.warnings,
            },
            selection: {
              range,
              waveId,
              instruction: selected,
              iteration,
              mode,
            },
          },
          null,
          2,
        ),
      ],
      { type: "application/json" },
    );
    const url = URL.createObjectURL(blob),
      a = document.createElement("a");
    a.href = url;
    a.download = "rocprof-selection.json";
    a.click();
    URL.revokeObjectURL(url);
  }
  const traceContext = { timeline, range, marks, setRange, waveGroup };
  return (
    <DisplayScaleContext.Provider value={displayScale}>
      <div className="app">
        <header className="topbar">
          <div className="brand">
            <div className="brand-icon">
              <Activity size={23} />
            </div>
            <b>
              rocprof<span>studio</span>
            </b>
            <span className="version">0.1 / PREVIEW</span>
          </div>
          <div className="top-actions">
            <span className="local">
              <i />
              {desktop ? "Local desktop" : "Browser preview"}
            </span>
            <button disabled={!desktop || busy} onClick={() => setAttSetup("")}>
              ATT setup
            </button>
            <button onClick={() => setHelp(true)} aria-label="Help">
              <CircleHelp size={17} />
            </button>
            <button
              disabled={!desktop || busy || !capture}
              onClick={saveWorkspace}
            >
              Save project
            </button>
            <button disabled={!desktop || busy} onClick={reopenWorkspace}>
              Open project
            </button>
            <button
              disabled={!desktop || busy || !capture || capture.id === "demo"}
              onClick={archiveCapture}
            >
              Archive capture
            </button>
            <button disabled={!capture} onClick={exportEvidence}>
              <Download size={15} />
              Export selection
            </button>
            <button
              disabled={
                !desktop ||
                busy ||
                !capture ||
                capture.id === "demo" ||
                !capture.realtime?.anchors.some((a) => a.se === targetSe) ||
                capture.counters.some((s) => s.spmClock)
              }
              title={`Calibrate to selected wave SE ${targetSe}, save a new combined archive and open it`}
              onClick={attachSpmCapture}
            >
              Attach SPM…
            </button>
            <button
              className="primary"
              disabled={busy}
              onClick={() => load(true)}
            >
              <FolderOpen size={15} />
              {busy
                ? archiveBusy
                  ? "Archiving…"
                  : "Importing…"
                : "Open capture"}
            </button>
            {busy && desktop && (
              <button
                onClick={() =>
                  void cancelImport().catch((e) => setError(String(e)))
                }
              >
                {archiveBusy ? "Cancel archive" : "Cancel import"}
              </button>
            )}
          </div>
        </header>
        <aside className="sidebar">
          <div className="workspace-name">
            <div className="project-icon">
              <Layers3 size={19} />
            </div>
            <div>
              <strong>Profiling workspace</strong>
              <span>Local analysis</span>
            </div>
          </div>
          <div className="eyebrow nav-label">ANALYZE</div>
          <nav>
            {views.map((v) => (
              <button
                key={v.id}
                className={view === v.id ? "active" : ""}
                onClick={() => setView(v.id)}
              >
                <v.icon size={17} />
                {v.name}
                {v.id === "evidence" && capture && (
                  <span className="nav-count">{capture.warnings.length}</span>
                )}
              </button>
            ))}
          </nav>
          <div className="sidebar-bottom">
            <label className="theme-control">
              Appearance
              <select
                aria-label="Color theme"
                value={appearance.theme}
                onChange={(e) =>
                  appearance.change(e.target.value as "dark" | "light")
                }
              >
                <option value="dark">Dark</option>
                <option value="light">Light</option>
              </select>
            </label>
            {appearance.error && <p role="status">{appearance.error}</p>}
            <label className="theme-control">
              Code font
              <select
                aria-label="Code font size"
                value={codeFont.size}
                onChange={(event) =>
                  codeFont.change(Number(event.target.value))
                }
              >
                {Array.from({ length: 15 }, (_, i) => i + 5).map((size) => (
                  <option key={size} value={size}>
                    {size} pt{size === 9 ? " (default)" : ""}
                  </option>
                ))}
              </select>
            </label>
            {codeFont.error && <p role="status">{codeFont.error}</p>}
            <label title="Applies to future decoded and raw ATT imports. Projects retain their saved choice; archives retain their packaged data.">
              <input type="checkbox" checked={waveStateImport.enabled}
                onChange={(e) => waveStateImport.change(e.target.checked)} />
              Load wave-state sidecars
            </label>
            {waveStateImport.error && <p role="status">{waveStateImport.error}</p>}
            <label
              className="theme-control"
              title="Adjusts timeline, line-chart and wait-link strokes; plot coordinates and text size stay fixed."
            >
              Plot strokes
              <select
                aria-label="Plot display scaling"
                value={displayScale.mode}
                onChange={(event) =>
                  displayScale.change(event.target.value as StrokeMode)
                }
              >
                <option value="system">System-scaled</option>
                <option value="device">Device pixels</option>
              </select>
            </label>
            {displayScale.error && <p role="status">{displayScale.error}</p>}

            <div className="future">
              <Sparkles size={17} />
              <strong>Kernel experiments</strong>
              <span>Isolation, tuning & AI iteration</span>
              <span className="badge muted">PLANNED</span>
            </div>
            <div className="sidebar-note">
              <ShieldCheck size={15} />
              <span>Evidence before optimization</span>
            </div>
          </div>
        </aside>
        <PlotAxesContext.Provider
          value={{
            axes: plotAxes,
            ranges: plotRanges,
            changeRange: (key, range) => {
              if (
                key.length > 8192 ||
                (range &&
                  !plotRanges[key] &&
                  Object.keys(plotRanges).length >= 4096)
              )
                throw Error("Saved plot range limit reached");
              setPlotRanges((previous) => {
                const next = { ...previous };
                if (range) next[key] = range;
                else delete next[key];
                return next;
              });
            },
            change: (key, bounds) => {
              if (key.length > 8192)
                throw Error("Plot identity exceeds the saved-axis limit");
              if (
                bounds &&
                !plotAxes[key] &&
                Object.keys(plotAxes).length >= 4096
              )
                throw Error(
                  "Saved-axis limit reached (4096 plots per capture)",
                );
              setPlotAxes((previous) => {
                const next = { ...previous };
                if (bounds) next[key] = bounds;
                else delete next[key];
                return next;
              });
            },
          }}
        >
          <DefinitionWorkspace.Provider
            value={{
              library: definitionLibrary,
              update: updateDefinitionLibrary,
              storageError: definitionStorageError,
              projectLocal: projectLibrary,
            }}
          >
            <main>
              <div className="capture-tabs">
                {captures.map((c) => (
                  <div
                    className={`capture-tab ${c.id === active ? "active" : ""}`}
                    key={c.id}
                  >
                    <button onClick={() => setActive(c.id)}>
                      <FileCode2 size={14} />
                      {c.name}
                      {c.metadata.demo && (
                        <span className="demo-label">DEMO</span>
                      )}
                    </button>
                    <button
                      aria-label={`Close ${c.name}`}
                      onClick={() => close(c.id)}
                    >
                      <X size={13} />
                    </button>
                  </div>
                ))}
                <button
                  className="add-tab"
                  title="Open capture file"
                  onClick={() => load(false)}
                >
                  <Plus size={16} />
                </button>
              </div>
              {error && (
                <div className="error-banner" role="alert">
                  <span>{error}</span>
                  <button
                    onClick={() => setError("")}
                    aria-label="Dismiss error"
                  >
                    <X size={15} />
                  </button>
                </div>
              )}
              {view === "reference" ? (
                <div className="content">
                  <IsaReference lookup={isaLookup} architecture={mcpIsa?.architecture} revision={mcpIsa?.revision} />
                </div>
              ) : capture ? (
                <>
                  <div className="page-heading">
                    <div>
                      <div className="breadcrumb">
                        Workspace <ChevronRight size={12} />
                        {capture.format}
                      </div>
                      <h1>{views.find((v) => v.id === view)?.name}</h1>
                      <p>
                        {view === "trace"
                          ? "Follow a wave from hardware execution to the instruction that caused it."
                          : view === "hardware"
                            ? "Make resource pressure visible, without confusing estimates with measurements."
                            : "Inspect the evidence behind your kernel’s behavior."}
                      </p>
                    </div>
                    <div className="capture-tags">
                      <span className="badge">
                        <span className="status-dot" />
                        {capture.architecture}
                      </span>
                      <span className="badge">
                        {capture.unit === "cycles"
                          ? "SQTT clock"
                          : "Timestamp clock"}
                      </span>
                    </div>
                  </div>
                  {capture.metadata.demo && (
                    <div className="demo-notice">
                      <Radio size={14} />
                      <span>
                        <b>Example capture</b> · Synthetic trace for exploring
                        the interface. These are not GPU measurements.
                      </span>
                      <button onClick={() => load(true)}>
                        Open your own <ChevronRight size={13} />
                      </button>
                    </div>
                  )}
                  {(dockLayout.pinned.length > 0 ||
                    !["summary", "sampling"].includes(view)) && (
                    <div className="range-toolbar">
                      <div>
                        <SlidersHorizontal size={15} />
                        <b>Visible range</b>
                        <RangeBound
                          key={`${capture.id}:${workspaceEpoch}:${timeline}:start`}
                          edge={0}
                          range={range}
                          bounds={[capture.start, capture.end]}
                          onCommit={setRange}
                        />
                        <span>→</span>
                        <RangeBound
                          key={`${capture.id}:${workspaceEpoch}:${timeline}:end`}
                          edge={1}
                          range={range}
                          bounds={[capture.start, capture.end]}
                          onCommit={setRange}
                        />
                        <span>{capture.unit}</span>
                        <button
                          title="Reset visible range"
                          onClick={() => setRange([capture.start, capture.end])}
                        >
                          <Maximize2 size={14} />
                        </button>
                      </div>
                      <span className="range-duration">
                        Δ {fmt(range[1] - range[0])} {capture.unit}
                      </span>
                    </div>
                  )}
                  {projectStatus && (
                    <div className="notice" role="status">
                      {projectStatus}
                    </div>
                  )}
                  <div className="content" key={workspaceEpoch}>
                    {!["summary", "sampling"].includes(view) &&
                      capture.waves.length > 0 && (
                        <div className="analysis-toolbar">
                          <label>
                            Latency contribution{" "}
                            <select
                              aria-label="Latency contribution"
                              value={nonhidden ? "nonhidden" : "total"}
                              onChange={(e) =>
                                setNonhidden(e.target.value === "nonhidden")
                              }
                            >
                              <option value="total">Total latency</option>
                              <option
                                value="nonhidden"
                                disabled={!hiddenResults[capture.id]}
                              >
                                Nonhidden latency
                              </option>
                            </select>
                          </label>
                          <label>
                            <input
                              type="checkbox"
                              checked={includeIdle}
                              onChange={(e) => setIncludeIdle(e.target.checked)}
                            />{" "}
                            Include idle in ISA
                          </label>
                          <button
                            disabled={hiddenBusy}
                            onClick={() => runHidden(capture)}
                          >
                            {hiddenBusy
                              ? "Analyzing…"
                              : hiddenResults[capture.id]
                                ? "Recompute hidden latency"
                                : "Analyze hidden latency"}
                          </button>
                          {hiddenResults[capture.id] && (
                            <span title={hiddenResults[capture.id].model}>
                              Pipe-priority estimate
                            </span>
                          )}
                        </div>
                      )}

                    {hiddenResults[capture.id]?.warnings?.map(
                      (warning, index) => (
                        <div
                          className="error-banner"
                          role="alert"
                          key={`hidden-warning-${index}`}
                        >
                          {warning}
                        </div>
                      ),
                    )}
                    {mode.startsWith("all") && !hiddenResults[capture.id] && !currentStats && (
                      visibleStatsError ? <div className="error-banner" role="alert">{visibleStatsError}</div> : <p role="status">Calculating exact visible-wave statistics…</p>
                    )}
                    {waveGroupError && (
                      <div className="error-banner" role="alert">
                        {waveGroupError}
                      </div>
                    )}
                    {waveGroupLoading && (
                      <p role="status">Loading detailed waves for the selected range…</p>
                    )}
                    {["cu", "util"].includes(timeline) && waveOverview?.capture === capture && waveOverview.data.range[0] === range[0] && waveOverview.data.range[1] === range[1] && !waveGroupLoading && (
                      <p role="status">{overviewDescription(waveOverview.data)}</p>
                    )}
                    <AnalysisPreferencesContext.Provider
                      value={{
                        values: analysisPreferences,
                        set: setAnalysisPreferences,
                      }}
                    >
                      <DockWorkspace
                        active={view}
                        layout={dockLayout}
                        onChange={setDockLayout}
                        views={[
                          ...views.filter((v) => v.id !== "reference"),
                          ...tracePanels,
                        ]}
                      >
                        {(view) => {
                          const detached = view.startsWith("trace-");
                          const timeline = detached
                            ? view.slice(6)
                            : traceContext.timeline;
                          const range =
                            detached &&
                            independentRanges &&
                            timeline !== traceContext.timeline
                              ? timelineRanges[timeline] || traceContext.range
                              : traceContext.range;
                          const marks = detached
                            ? pinnedMarks[timeline] || []
                            : traceContext.marks;
                          const waveGroup =
                            detached && timeline !== traceContext.timeline
                              ? pinnedWaves.capture === capture
                                ? pinnedWaves.groups[timeline] || []
                                : []
                              : traceContext.waveGroup;
                          const setRange = (next: Range) => {
                            if (
                              detached &&
                              independentRanges &&
                              timeline !== traceContext.timeline
                            )
                              setTimelineRanges((previous) => ({
                                ...previous,
                                [timeline]: next,
                              }));
                            else traceContext.setRange(next);
                          };
                          return (
                            <>
                              {(view === "trace" || detached) && (
                                <>
                                  {!detached && (
                                    <div className="metrics">
                                      <Metric
                                        label="Recorded wave instances"
                                        value={fmt(capture.waves.length)}
                                        detail="Detailed traces · target CU only"
                                      />
                                      <Metric
                                        label="ISA instructions"
                                        value={fmt(capture.instructions.length)}
                                        detail="Source-linked where symbols exist"
                                      />
                                      <Metric
                                        label="Capture span"
                                        value={`${fmt(capture.end - capture.start)} ${capture.unit}`}
                                        detail="Instrumented capture window"
                                      />
                                      <Metric
                                        label="Coverage status"
                                        value="Unqualified"
                                        detail="Independent replay check required"
                                      />
                                    </div>
                                  )}
                                  <Panel
                                    title={
                                      detached
                                        ? tracePanels.find(
                                            (p) => p.id === view,
                                          )!.name
                                        : "Execution timeline"
                                    }
                                    detail={
                                      independentRanges
                                        ? "Each timeline keeps its own range. Other analysis views follow the currently selected timeline range."
                                        : "One clock range shared across analysis views."
                                    }
                                    tools={
                                      !detached && (
                                        <div className="segmented">
                                          {[
                                            ["wave", "Target wave"],
                                            ["cu", "CU waves"],
                                            ["util", "Utilization"],
                                            ["global", "Global"],
                                            ["dispatch", "Dispatches"],
                                          ].map(([id, label]) => (
                                            <button
                                              key={id}
                                              className={
                                                timeline === id ? "active" : ""
                                              }
                                              onClick={() => changeTimeline(id)}
                                            >
                                              {label}
                                            </button>
                                          ))}
                                        </div>
                                      )
                                    }
                                  >
                                    <div className="wave-toolbar">
                                      <label>
                                        <input
                                          type="checkbox"
                                          aria-label="Link timeline ranges"
                                          checked={!independentRanges}
                                          onChange={(e) =>
                                            changeRangeLink(!e.target.checked)
                                          }
                                        />{" "}
                                        Link timeline ranges
                                      </label>
                                      <span className="eyebrow">
                                        TARGET WAVE
                                      </span>
                                      {(
                                        ["se", "simd", "slot", "wid"] as const
                                      ).map((field, i) => (
                                        <label key={field}>
                                          {["SE", "SIMD", "Slot", "Wave ID"][i]}
                                          <select
                                            aria-label={
                                              [
                                                "Shader engine",
                                                "SIMD",
                                                "Slot",
                                                "Wave ID",
                                              ][i]
                                            }
                                            value={windex?.[field] ?? ""}
                                            disabled={!capture.waves.length}
                                            onChange={(e) =>
                                              filterWave(
                                                field,
                                                Number(e.target.value),
                                              )
                                            }
                                          >
                                            {[
                                              ...new Set(
                                                capture.waves
                                                  .filter(
                                                    (w) =>
                                                      (field === "se" ||
                                                        w.se === windex?.se) &&
                                                      (field === "se" ||
                                                        field === "simd" ||
                                                        w.simd ===
                                                          windex?.simd) &&
                                                      (field !== "wid" ||
                                                        w.slot ===
                                                          windex?.slot),
                                                  )
                                                  .map((w) => w[field]),
                                              ),
                                            ]
                                              .sort((a, b) => a - b)
                                              .map((v) => (
                                                <option key={v}>{v}</option>
                                              ))}
                                          </select>
                                        </label>
                                      ))}
                                      <button
                                        disabled={!wave}
                                        onClick={() =>
                                          wave &&
                                          setRange([wave.start, wave.end])
                                        }
                                      >
                                        Fit wave
                                      </button>
                                      <div className="legend">
                                        {[6, 3, 5, 9].map((i) => (
                                          <span key={i}>
                                            <i
                                              style={{ background: colors[i] }}
                                            />
                                            {categories[i]}
                                          </span>
                                        ))}
                                      </div>
                                    </div>
                                    {timeline === "global" && (
                                      <div className="source-remap-controls">
                                        {(
                                          [
                                            "residency",
                                            "markers",
                                            "dispatches",
                                            "events",
                                          ] as const
                                        ).map((layer) => (
                                          <label key={layer}>
                                            <input
                                              type="checkbox"
                                              aria-label={`Show global ${layer}`}
                                              checked={globalLayers[layer]}
                                              onChange={(e) =>
                                                setGlobalLayers((previous) => ({
                                                  ...previous,
                                                  [layer]: e.target.checked,
                                                }))
                                              }
                                            />{" "}
                                            {layer}
                                          </label>
                                        ))}
                                        <details>
                                          <summary>Event groups</summary>
                                          {decoderEventGroups.map(
                                            ([bit, label]) => (
                                              <label key={bit}>
                                                <input
                                                  type="checkbox"
                                                  aria-label={`Decoder group ${label}`}
                                                  checked={
                                                    !!(
                                                      (globalLayers.eventGroups ??
                                                        defaultEventGroups) &
                                                      bit
                                                    )
                                                  }
                                                  onChange={(e) =>
                                                    setGlobalLayers(
                                                      (previous) => ({
                                                        ...previous,
                                                        eventGroups: e.target
                                                          .checked
                                                          ? (previous.eventGroups ??
                                                              defaultEventGroups) |
                                                            bit
                                                          : (previous.eventGroups ??
                                                              defaultEventGroups) &
                                                            ~bit,
                                                      }),
                                                    )
                                                  }
                                                />{" "}
                                                {label}
                                              </label>
                                            ),
                                          )}
                                        </details>
                                        <span>
                                          Shared capture clock. Dispatch tracks
                                          do not imply a wave association.
                                        </span>
                                      </div>
                                    )}
                                    {timeline === "cu" && (
                                      <div className="source-remap-controls">
                                        <label>
                                          <input
                                            type="checkbox"
                                            aria-label="Show CU events"
                                            checked={cuEvents}
                                            onChange={(e) =>
                                              setCuEvents(e.target.checked)
                                            }
                                          />{" "}
                                          Decoder events
                                        </label>
                                        <details>
                                          <summary>CU event groups</summary>
                                          {decoderEventGroups.map(
                                            ([bit, label]) => (
                                              <label key={bit}>
                                                <input
                                                  type="checkbox"
                                                  aria-label={`CU decoder group ${label}`}
                                                  checked={
                                                    !!(cuEventGroups & bit)
                                                  }
                                                  onChange={(e) =>
                                                    setCuEventGroups(
                                                      (previous) =>
                                                        e.target.checked
                                                          ? previous | bit
                                                          : previous & ~bit,
                                                    )
                                                  }
                                                />{" "}
                                                {label}
                                              </label>
                                            ),
                                          )}
                                        </details>
                                        <span>
                                          Selected SE; upstream CU exclusions
                                          omit SPM samples, code-object changes
                                          and CS partial flush.
                                        </span>
                                      </div>
                                    )}
                                    {(timeline === "global" ||
                                      timeline === "dispatch" ||
                                      timeline === "cu" ||
                                      timeline === "util") &&
                                      globalSelection?.capture === capture && (
                                        <div
                                          className="source-remap-controls"
                                          role="status"
                                          aria-label="Timeline record details"
                                        >
                                          <span>
                                            {globalSelection.mark.label} ·{" "}
                                            {globalSelection.mark.start}–
                                            {globalSelection.mark.end}{" "}
                                            {capture.unit}
                                          </span>
                                          {globalSelection.mark.recordKind === "dispatch" && <button onClick={() => {
                                            const selectedDispatch = globalSelection.mark.recordIndex === undefined ? undefined : capture.dispatches[globalSelection.mark.recordIndex];
                                            if (selectedDispatch) void tuningInvoke("open_tuning", {captureId:capture.id,dispatchId:selectedDispatch.id}).catch(e=>setError(String(e)));
                                          }}>Tune this kernel</button>}
                                          {globalSelection.mark.recordKind ===
                                            "marker" && (
                                            <button
                                              onClick={() => setView("markers")}
                                            >
                                              Open marker analysis
                                            </button>
                                          )}
                                        </div>
                                      )}
                                    {detached &&
                                      pinnedWaves.capture === capture && pinnedWaves.pending[timeline] && (
                                        <p role="status">Loading detailed waves for this panel…</p>
                                      )}
                                    {detached && pinnedWaves.capture === capture && !pinnedWaves.pending[timeline] && pinnedWaves.overviews[timeline] &&
                                      pinnedWaves.overviews[timeline].range[0] === range[0] && pinnedWaves.overviews[timeline].range[1] === range[1] && (
                                        <p role="status">{overviewDescription(pinnedWaves.overviews[timeline])}</p>
                                      )}
                                    {detached &&
                                      pinnedWaves.capture === capture &&
                                      pinnedWaves.errors[timeline] && (
                                        <div className="notice" role="alert">
                                          {pinnedWaves.errors[timeline]}
                                        </div>
                                      )}
                                    {marks.length ||
                                    capture.waves.length > 0 ? (
                                      <Timeline
                                        key={timeline}
                                        preferenceId={timeline}
                                        preferences={
                                          timelinePreferences[timeline]
                                        }
                                        onPreferences={(preferences) =>
                                          setTimelinePreferences(
                                            (previous) => ({
                                              ...previous,
                                              [timeline]: preferences,
                                            }),
                                          )
                                        }
                                        exportLabel={`${capture.name} · ${timeline}`}
                                        marks={marks}
                                        loadExportMarks={async (window, signal) => {
                                          if (
                                            timeline === "cu" ||
                                            timeline === "util"
                                          ) {
                                            const group = await readWaveWindow(
                                              capture,
                                              targetSe,
                                              window[0],
                                              window[1],
                                              signal,
                                            );
                                            return buildMarks(timeline, group);
                                          }
                                          return marks;
                                        }}
                                        range={range}
                                        bounds={[capture.start, capture.end]}
                                        onRange={setRange}
                                        unit={capture.unit}
                                        baseClock={
                                          timeline === "wave" ||
                                          timeline === "cu" ||
                                          timeline === "util"
                                            ? utilizationModel?.base
                                            : undefined
                                        }
                                        cursor={
                                          waitIndex.tokenAt(selected, iteration)
                                            ?.start
                                        }
                                        onSelect={(m) => {
                                          globalWaveSearch.current?.abort();
                                          if (
                                            m.recordKind === "dispatch" ||
                                            m.recordKind === "marker" ||
                                            m.recordKind === "event" ||
                                            m.recordKind === "activity"
                                          ) {
                                            setGlobalSelection({
                                              capture,
                                              mark: m,
                                            });
                                            setRange(
                                              boundedRange(
                                                [m.start, m.end],
                                                [capture.start, capture.end],
                                              ),
                                            );
                                            return;
                                          }
                                          if (timeline === "global") {
                                            void openGlobalWave(m);
                                            return;
                                          }
                                          if (
                                            m.waveId &&
                                            m.tokenIndex !== undefined
                                          ) {
                                            if (m.instruction !== undefined && m.tokenOccurrence !== undefined) {
                                              select(m.instruction, m.waveId, m.tokenOccurrence, timeline, range);
                                              return;
                                            }
                                            const current =
                                              m.waveId === wave?.id
                                                ? wave
                                                : waveGroup.find(
                                                    (w) => w.id === m.waveId,
                                                  );
                                            const token =
                                              current?.tokens[m.tokenIndex];
                                            if (token && current) {
                                              select(
                                                token.instruction,
                                                current.id,
                                                tokenIteration(
                                                  current.tokens,
                                                  m.tokenIndex,
                                                ),
                                                timeline,
                                                range,
                                              );
                                            }
                                          }
                                        }}
                                      />
                                    ) : (
                                      <Empty
                                        title="No records for this view"
                                        detail={
                                          capture.waves.length
                                            ? "Choose a different timeline or target wave."
                                            : "This capture has no detailed wave tokens. Use Global, Dispatches, Counters or Kernel dispatches as applicable."
                                        }
                                      />
                                    )}{" "}
                                    {!marks.length &&
                                      capture.waves.length > 0 && (
                                        <Empty
                                          title="No records for this view"
                                          detail="No records match the current range and layer filters. Adjust the range, filters or target wave; timeline controls remain available."
                                        />
                                      )}
                                    {timeline === "util" && (
                                      <div className="notice">
                                        {utilizationModel ? (
                                          <>
                                            Upstream issue projection · minimum
                                            issue quantum{" "}
                                            {utilizationModel.base} shader clock
                                            {utilizationModel.base === 1
                                              ? ""
                                              : "s"}
                                            . Stalls are clamped to retain that
                                            quantum; IMMED / TRAP / MSG use at
                                            most one quantum. Raw timestamps are
                                            unchanged. Recorded waves only, not
                                            measured utilization percentages.
                                            Unknown token categories are
                                            omitted.
                                            {utilizationModel.family ===
                                              "gcn" && (
                                              <label>
                                                {" "}
                                                <input
                                                  type="checkbox"
                                                  checked={separateLds}
                                                  onChange={(e) =>
                                                    setSeparateLds(
                                                      e.target.checked,
                                                    )
                                                  }
                                                />
                                                Separate LDS pipe
                                              </label>
                                            )}
                                          </>
                                        ) : (
                                          "Issue projection is unavailable: the capture has no recognized architecture or gfxip clock model."
                                        )}
                                      </div>
                                    )}
                                  </Panel>
                                  {!detached && (
                                    <>
                                      <div className="isa-controls">
                                        <div>
                                          <button
                                            aria-label="Previous selection"
                                            disabled={historyIndex <= 0}
                                            onClick={() => {
                                              restoreHistory(historyIndex - 1);
                                            }}
                                          >
                                            <ArrowLeft size={15} />
                                          </button>
                                          <button
                                            aria-label="Next selection"
                                            disabled={
                                              historyIndex >= history.length - 1
                                            }
                                            onClick={() => {
                                              restoreHistory(historyIndex + 1);
                                            }}
                                          >
                                            <ArrowRight size={15} />
                                          </button>
                                          <span>
                                            Instruction <b>{selected}</b>
                                          </span>
                                        </div>
                                        <label>
                                          Latency scope{" "}
                                          <select
                                            aria-label="Latency scope"
                                            value={mode}
                                            onChange={(e) =>
                                              setMode(e.target.value)
                                            }
                                          >
                                            <option value="capture">
                                              Whole capture · sum
                                            </option>
                                            <option value="capture-mean">
                                              Whole capture · mean
                                            </option>
                                            <option value="max">
                                              Target wave · maximum
                                            </option>
                                            <option value="iteration">
                                              Target wave · selected occurrence
                                            </option>
                                            <option value="wave">
                                              Target wave · visible range · sum
                                            </option>
                                            <option value="all">
                                              Selected SE · visible waves · sum
                                            </option>
                                            <option value="all-mean">
                                              Selected SE · visible waves · mean
                                            </option>
                                            <option value="mean">
                                              Target wave · visible range · mean
                                            </option>
                                          </select>
                                        </label>
                                      </div>
                                      <Instructions
                                        state={isaState}
                                        onState={setIsaState}
                                        sourceMappings={
                                          sourceMappings[capture.id] || {}
                                        }
                                        onMapBatch={(files) => {
                                          if (
                                            !capturesRef.current.some(
                                              (c) => c.id === capture.id,
                                            )
                                          )
                                            return;
                                          const next = mapSourceBatch(
                                            sourceMappingsRef.current[
                                              capture.id
                                            ] || {},
                                            files,
                                            [
                                              ...(capture.sourceFiles || []),
                                              ...Object.keys(capture.sources),
                                            ],
                                          );
                                          const updated = {
                                            ...sourceMappingsRef.current,
                                            [capture.id]: next,
                                          };
                                          sourceMappingsRef.current = updated;
                                          setSourceMappings(updated);
                                        }}
                                        onMapSource={(path, file) => {
                                          if (
                                            !capturesRef.current.some(
                                              (c) => c.id === capture.id,
                                            )
                                          )
                                            return;
                                          const next = mapSource(
                                            sourceMappingsRef.current[
                                              capture.id
                                            ] || {},
                                            path,
                                            file,
                                          );
                                          const updated = {
                                            ...sourceMappingsRef.current,
                                            [capture.id]: next,
                                          };
                                          sourceMappingsRef.current = updated;
                                          setSourceMappings(updated);
                                        }}
                                        onDocumentation={(text) => {
                                          setIsaLookup(text);
                                          setView("reference");
                                        }}
                                        capture={capture}
                                        instructions={instructions}
                                        sourceInstructions={sourceInstructions}
                                        hasHiddenCosts={
                                          !!hiddenResults[capture.id]
                                        }
                                        wave={wave}
                                        waitIndex={waitIndex}
                                        selected={selected}
                                        onSelect={select}
                                        onTokenSelect={(index) => {
                                          const location =
                                            waitIndex.location(index);
                                          if (location)
                                            select(
                                              location.instruction,
                                              location.waveId,
                                              location.iteration,
                                              "wave",
                                            );
                                        }}
                                        highlighted={highlighted}
                                        iteration={iteration}
                                        onIteration={chooseIteration}
                                      />
                                    </>
                                  )}
                                </>
                              )}
                              {view === "sampling" && (
                                <PcSampling
                                  key={capture.id}
                                  capture={capture}
                                  onSelect={jump}
                                />
                              )}
                              {view === "summary" && (
                                <SummaryView
                                  key={capture.id}
                                  capture={capture}
                                />
                              )}
                              {view === "hardware" && (
                                <HardwareView capture={capture} range={range} />
                              )}
                              {view === "occupancy" && (
                                <OccupancyView
                                  capture={capture}
                                  range={range}
                                />
                              )}
                              {view === "dispatch" && (
                                <DispatchView
                                  hostPreferences={
                                    timelinePreferences["host-calls"]
                                  }
                                  onHostPreferences={(preferences) =>
                                    setTimelinePreferences((previous) => ({
                                      ...previous,
                                      "host-calls": preferences,
                                    }))
                                  }
                                  capture={capture}
                                  range={range}
                                  onRange={setRange}
                                  onTune={(dispatch) => { void tuningInvoke("open_tuning", {captureId: capture.id, dispatchId: dispatch.id}).catch(e => setError(String(e))); }}
                                />
                              )}
                              {(view === "hotspots" || view === "flame") && (
                                <>
                                  <div className="notice">
                                    Scope:{" "}
                                    {mode === "capture"
                                      ? "whole capture"
                                      : mode.startsWith("all")
                                        ? "selected SE · visible waves"
                                        : "target wave · visible range"}
                                    . Histogram and flamegraph weights use
                                    accumulated latency, irrespective of the ISA
                                    mean/sum display setting.
                                  </div>
                                  {view === "hotspots" ? (
                                    <HotspotView
                                      bins={histogramBins}
                                      onBins={setHistogramBins}
                                      instructions={summedInstructions}
                                      exportContext={`${capture.name} · ${mode === "capture" ? "whole capture" : `${mode} · SE ${targetSe} · ${range[0]}–${range[1]} ${capture.unit}`} · ${nonhidden && hiddenResults[capture.id] ? "nonhidden" : "total"} latency`}
                                      onSelect={jump}
                                      onSelectGroup={jumpGroup}
                                    />
                                  ) : (
                                    <FlameView
                                      instructions={summedInstructions}
                                      exportContext={`${capture.name} · ${mode === "capture" ? "whole capture" : `${mode} · SE ${targetSe} · ${range[0]}–${range[1]} ${capture.unit}`} · ${nonhidden && hiddenResults[capture.id] ? "nonhidden" : "total"} latency`}
                                      onSelect={jump}
                                    />
                                  )}
                                </>
                              )}
                              {view === "markers" && (
                                <MarkersView
                                  includeIdle={includeIdle}
                                  hidden={
                                    nonhidden
                                      ? hiddenResults[capture.id]
                                      : undefined
                                  }
                                  preferences={timelinePreferences.markers}
                                  onPreferences={(preferences) =>
                                    setTimelinePreferences((previous) => ({
                                      ...previous,
                                      markers: preferences,
                                    }))
                                  }
                                  key={capture.id}
                                  capture={capture}
                                  range={range}
                                  onRange={setRange}
                                  onSource={jump}
                                />
                              )}
                              {view === "counters" && (
                                <CountersView
                                  onDispatch={(dispatch) => {
                                    setRange(
                                      boundedRange(
                                        [dispatch.start, dispatch.end],
                                        [capture.start, capture.end],
                                      ),
                                    );
                                    setView("dispatch");
                                  }}
                                  definitions={scalarCounters}
                                  onDefinitions={setScalarCounters}
                                  key={capture.id}
                                  capture={capture}
                                  range={range}
                                />
                              )}
                              {view === "evidence" && (
                                <EvidenceView
                                  capture={capture}
                                  onOpenTrace={(path) => void loadRun({ path })}
                                  onExport={exportDiagnosticManifest}
                                  exporting={diagnosticBusy}
                                  exportStatus={diagnosticStatus}
                                />
                              )}
                            </>
                          );
                        }}
                      </DockWorkspace>
                    </AnalysisPreferencesContext.Provider>
                  </div>
                </>
              ) : (
                <div className="welcome">
                  <Activity size={48} />
                  <h1>Your next optimization starts with a trace.</h1>
                  <p>
                    Open an existing rocprofv3 capture, or explore the example
                    workspace.
                  </p>
                  <button className="primary" onClick={() => load(true)}>
                    <FolderOpen size={17} />
                    Open capture directory
                  </button>
                  <button
                    onClick={() =>
                      example().then((c) => {
                        setCaptures((cs) => [...cs, c]);
                        setActive("demo");
                      })
                    }
                  >
                    Explore example
                  </button>
                </div>
              )}
              <footer>
                <span>
                  <i className="status-dot" />{" "}
                  {busy
                    ? archiveBusy
                      ? "Archiving capture…"
                      : "Importing capture…"
                    : "Ready"}{" "}
                  <span className="footer-divider">/</span>{" "}
                  {capture?.format || "No capture"}
                </span>
                <span>
                  Rust analysis core <span className="footer-divider">·</span>{" "}
                  {desktop ? "Tauri 2" : "Desktop import available in Tauri"}{" "}
                  <span className="footer-divider">·</span> No live collection
                  running
                </span>
              </footer>
            </main>
          </DefinitionWorkspace.Provider>
        </PlotAxesContext.Provider>
        {attSetup !== null && (
          <AttSetup
            initialPath={attSetup}
            onCancel={() => setAttSetup(null)}
            onReady={(path) => {
              setAttSetup(null);
              void loadRun({ path });
            }}
          />
        )}
        {samplingChoice && (
          <SamplingChooser onSelect={(samplingMethod) => void loadRun({path: samplingChoice, samplingMethod})} onCancel={() => setSamplingChoice(null)} />
        )}
        {runChoices && (
          <RunChooser
            runs={runChoices}
            onSelect={loadRun}
            onCancel={() => setRunChoices(null)}
          />
        )}
        {help && (
          <div className="modal-backdrop" onClick={() => setHelp(false)}>
            <div className="modal" onClick={(e) => e.stopPropagation()}>
              <button className="modal-close" onClick={() => setHelp(false)}>
                <X />
              </button>
              <h2>A trace, connected.</h2>
              <p>
                Open a decoded <code>ui_output_*</code> directory, a rocprofv3
                kernel trace CSV, SPM results JSON, or PC sampling JSON/CSV in the desktop app.
              </p>
              <p>
                Scroll on timelines to zoom. Drag to pan; Shift-drag or
                right-drag to measure. Use A/D for keyboard panning. Click
                tokens, ISA rows, or source lines to navigate.
              </p>
              <p>
                Before collecting SPM, check both configurations and counters:
                <code> rocprofv3-avail list --spm-config --spm</code>. An
                advertised sampling interval does not guarantee that the
                installed stack supports any SPM counters on that GPU.
              </p>
              <p>
                The sample is synthetic. Coverage, spills and bank conflicts
                remain unknown unless supported by capture evidence. See the
                feature parity checklist in the project documentation for
                remaining upstream functionality.
              </p>
              <button className="primary" onClick={() => setHelp(false)}>
                Back to the trace
              </button>
            </div>
          </div>
        )}
      </div>
    </DisplayScaleContext.Provider>
  );
}
