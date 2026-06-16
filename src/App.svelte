<script lang="ts">
    import { onMount } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";
    import { writeText } from "@tauri-apps/plugin-clipboard-manager";
    import { getCurrentWindow } from "@tauri-apps/api/window";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { theme, defaultTheme, type ThemeVars } from "./lib/stores/theme";

    type Tab = "library" | "uploads" | "permanent" | "settings" | "editor";
    type ToastKind = "ok" | "err";
    type ViewMode = "grid" | "list";
    type LibraryFilter = "all" | "local" | "uploaded" | "temporary" | "permanent" | "favorite";
    type LibrarySort = "newest" | "oldest" | "longest" | "shortest" | "name";
    type SelectOption<T extends string = string> = {
        value: T;
        label: string;
    };
    type BootPhase = "terminal" | "ready";
    type UploadStatus =
        | "queued"
        | "uploading"
        | "uploaded"
        | "failed"
        | "canceled";

    type Clip = {
        id: string;
        filename: string;
        window_name: string | null;
        path: string;
        duration: number;
        created_at: string;
        thumbnail_path: string | null;
        tags: string | null;
        folder: string | null;
        upload_status: string;
        r2_key: string | null;
        r2_url: string | null;
        expiry_date: string | null;
        is_permanent: boolean;
        favorite: boolean;
    };

    type UploadQueueItem = {
        queueId: string;
        clipId: string;
        filename: string;
        permanent: boolean;
        status: UploadStatus;
        progress: number;
        message: string;
        url: string | null;
        error: string | null;
        autoCopy: boolean;
        createdAt: number;
    };
    type AudioTrack = {
        index: number;
        key: string;
        label: string;
        isolated: boolean;
    };

    let tab = $state<Tab>("library");
    let clips = $state<Clip[]>([]);
    let uploadedClips = $state<Clip[]>([]);
    let permanentClips = $state<Clip[]>([]);
    let editClip = $state<Clip | null>(null);
    let settings = $state<any>({});
    let recState = $state({
        replay_buffer_active: false,
        recording_active: false,
    });
    let storageUsage = $state(0);
    let thumbCache = $state<Record<string, string>>({});
    let viewMode = $state<ViewMode>("grid");
    let sidebarCollapsed = $state(false);
    let librarySearch = $state("");
    let libraryFilter = $state<LibraryFilter>("all");
    let librarySort = $state<LibrarySort>("newest");
    let openDropdown = $state<string | null>(null);
    let audioInputOptions = $state<SelectOption[]>([
        { value: "default_input", label: "Default input" },
    ]);
    let audioInputDevicesLoading = $state(false);
    let bootPhase = $state<BootPhase>("terminal");
    let bootProgress = $state(0);
    let bootTerminalLines = $state<string[]>([]);
    let uploadQueue = $state<UploadQueueItem[]>([]);
    let uploadQueueOpen = $state(false);
    let filteredClips = $derived.by(() => {
        const query = librarySearch.trim().toLowerCase();
        return [...clips]
            .filter((clip) => {
                if (libraryFilter === "local" && clip.r2_url) return false;
                if (libraryFilter === "uploaded" && !clip.r2_url) return false;
                if (libraryFilter === "temporary" && (!clip.r2_url || clip.is_permanent)) return false;
                if (libraryFilter === "permanent" && (!clip.r2_url || !clip.is_permanent)) return false;
                if (libraryFilter === "favorite" && !clip.favorite) return false;
                if (!query) return true;

                return [clip.filename, clip.window_name, clip.tags, clip.folder]
                    .filter(Boolean)
                    .some((value) => value!.toLowerCase().includes(query));
            })
            .sort((a, b) => {
                if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
                if (librarySort === "oldest") {
                    return new Date(a.created_at).getTime() - new Date(b.created_at).getTime();
                }
                if (librarySort === "longest") return b.duration - a.duration;
                if (librarySort === "shortest") return a.duration - b.duration;
                if (librarySort === "name") return a.filename.localeCompare(b.filename);
                return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
            });
    });
    let activeUploadCount = $derived(
        uploadQueue.filter(
            (item) => item.status === "queued" || item.status === "uploading",
        ).length,
    );
    let failedUploadCount = $derived(
        uploadQueue.filter((item) => item.status === "failed").length,
    );
    let settledUploadCount = $derived(
        uploadQueue.filter(
            (item) => item.status !== "queued" && item.status !== "uploading",
        ).length,
    );

    let toast = $state<{ msg: string; kind: ToastKind } | null>(null);
    let toastTimer: any;
    let uploadWorkerActive = false;
    const uploadTimers = new Map<string, ReturnType<typeof setInterval>>();

    let r2TempBytes = $state(0);
    let r2PermBytes = $state(0);

    let videoEl: HTMLVideoElement;
    let previewSrc = $state("");
    let previewLoading = $state(false);
    let currentTime = $state(0);
    let duration = $state(0);
    let trimStart = $state(0);
    let trimEnd = $state(10);
    let editorAudioTracks = $state<AudioTrack[]>([]);
    let editorMutedAudioTracks = $state<Record<number, boolean>>({});

    // Theme editor state
    let themeExpanded = $state(false);
    let currentTheme = $state<ThemeVars>({ ...defaultTheme });

    const colorVars: { key: keyof ThemeVars; label: string }[] = [
        { key: "--bg-base", label: "bg_base" },
        { key: "--bg-deepest", label: "bg_deepest" },
        { key: "--bg-elev-1", label: "bg_elevated" },
        { key: "--bg-elev-2", label: "bg_surface" },
        { key: "--text", label: "text" },
        { key: "--text-dim", label: "text_dim" },
        { key: "--text-muted", label: "text_muted" },
        { key: "--text-faint", label: "text_faint" },
        { key: "--border", label: "border" },
        { key: "--border-strong", label: "border_hover" },
        { key: "--accent", label: "accent" },
        { key: "--danger", label: "danger" },
    ];

    const libraryFilterOptions: SelectOption<LibraryFilter>[] = [
        { value: "all", label: "all" },
        { value: "local", label: "local" },
        { value: "uploaded", label: "uploaded" },
        { value: "temporary", label: "temp" },
        { value: "permanent", label: "perm" },
        { value: "favorite", label: "pinned" },
    ];

    const librarySortOptions: SelectOption<LibrarySort>[] = [
        { value: "newest", label: "newest" },
        { value: "oldest", label: "oldest" },
        { value: "longest", label: "longest" },
        { value: "shortest", label: "shortest" },
        { value: "name", label: "name" },
    ];

    const codecOptions: SelectOption[] = [
        { value: "h264", label: "h264" },
        { value: "hevc", label: "hevc" },
        { value: "av1", label: "av1" },
    ];

    const containerOptions: SelectOption[] = [
        { value: "mp4", label: "mp4" },
        { value: "mkv", label: "mkv" },
        { value: "webm", label: "webm" },
    ];

    const audioCodecOptions: SelectOption[] = [
        { value: "aac", label: "aac" },
        { value: "opus", label: "opus" },
        { value: "flac", label: "flac" },
    ];

    const bootTerminalScript = [
        "$ klyppd --session",
        ":: initializing terminal session",
        ":: resolving clip cache",
        ":: probing recorder backend",
        ":: preparing library index",
        ":: handing off to loader",
    ];
    const minimumBootMs = 900;
    const bootLineDelayMs = 80;

    // ─── Lifecycle ─────────────────────────────────────────────────────────

    onMount(async () => {
        const bootStart = performance.now();
        const terminalTask = playBootTerminal();

        try {
            await bootStep(12, async () => {
                await injectUserTheme();
                theme.apply();
            });
            await bootStep(26, async () => {
                settings = await invoke("get_settings");
            });
            await bootStep(42, async () => {
                recState = await invoke("get_recording_state");
            });
            await bootStep(56, async () => {
                storageUsage = await invoke("get_storage_usage");
            });
            await bootStep(78, loadClips);
            await bootStep(92, async () => {
                listen("clip-saved", (e: any) => {
                    notify("Clip saved: " + e.payload.filename);
                    setTimeout(loadClips, 500);
                });
                listen("toast", (e: any) => notify(e.payload.msg, e.payload.kind));

                // Subscribe to theme store for local reactivity
                theme.subscribe((v) => {
                    currentTheme = v;
                });
            });

            bootProgress = 100;
        } catch (e: any) {
            console.error(e);
            notify("Startup failed", "err");
        } finally {
            setInterval(refresh, 3000);
            await Promise.allSettled([
                terminalTask,
                holdBoot(bootStart, minimumBootMs),
            ]);
            bootProgress = 100;
            await sleep(120);
            bootPhase = "ready";
            forceRepaint();
        }
    });

    async function playBootTerminal() {
        bootTerminalLines = [];
        for (const line of bootTerminalScript) {
            bootTerminalLines = [...bootTerminalLines, line];
            await sleep(bootLineDelayMs);
        }
        await sleep(120);
    }

    async function bootStep(progress: number, task: () => Promise<void>) {
        bootProgress = Math.max(bootProgress, progress - 8);
        await task();
        bootProgress = Math.max(bootProgress, progress);
    }

    function sleep(ms: number) {
        return new Promise<void>((resolve) => setTimeout(resolve, ms));
    }

    async function holdBoot(start: number, minimumMs: number) {
        const remaining = minimumMs - (performance.now() - start);
        if (remaining > 0) await sleep(remaining);
    }

    async function injectUserTheme() {
        const css = await invoke<string>("get_theme_css");
        if (!css) return;
        theme.importCSS(css);
        await applyWindowOpacity(theme.current()["--bg-opacity"]);
    }

    async function persistTheme() {
        try {
            await invoke("save_theme_css", { css: theme.exportCSS() });
        } catch (e) {
            console.error(e);
            notify("Theme save failed", "err");
        }
    }

    async function applyWindowOpacity(value: string) {
        const opacity = Number.parseFloat(value);
        if (!Number.isFinite(opacity)) return;
        try {
            await invoke("set_window_opacity", { opacity });
        } catch (e) {
            console.error(e);
        }
    }

    // ─── Notifications ─────────────────────────────────────────────────────

    function notify(msg: string, kind: ToastKind = "ok") {
        toast = { msg, kind };
        clearTimeout(toastTimer);
        toastTimer = setTimeout(() => (toast = null), 2500);
    }

    // ─── Rendering workaround ───────────────────────────────────────────────

    function forceRepaint() {
        requestAnimationFrame(() => {
            const el = document.querySelector("main");
            if (el) {
                el.style.opacity = "0.999";
                requestAnimationFrame(() => {
                    el.style.opacity = "";
                });
            }
        });
    }

    // ─── Data fetching ─────────────────────────────────────────────────────

    async function refresh() {
        recState = await invoke("get_recording_state");
        if (tab !== "settings") settings = await invoke("get_settings");
        storageUsage = await invoke("get_storage_usage");
        if (tab === "uploads" || tab === "permanent") {
            await refreshR2TabState();
        }
    }

    async function loadClips() {
        clips = await invoke<Clip[]>("scan_clips");
        for (const c of clips) {
            if (c.thumbnail_path) loadThumb(c.thumbnail_path);
        }
    }

    async function loadThumb(path: string) {
        if (thumbCache[path]) return;
        try {
            thumbCache[path] = await invoke<string>("read_thumbnail", { path });
        } catch {}
    }

    async function switchTab(t: Tab) {
        tab = t;
        // Force webkit2gtk repaint (transparent bg rendering bug)
        forceRepaint();
        if (t === "library") await loadClips();
        if (t === "uploads" || t === "permanent") {
            await refreshR2TabState();
        }
        if (t === "settings") await loadAudioInputDevices();
    }

    async function refreshR2TabState() {
        if (tab === "uploads") {
            uploadedClips = await invoke("get_uploaded_clips", {
                permanent: false,
            });
            for (const c of uploadedClips)
                if (c.thumbnail_path) loadThumb(c.thumbnail_path);
            r2TempBytes = await invoke<number>("r2_storage", {
                permanent: false,
            }).catch(() => 0);
        }
        if (tab === "permanent") {
            permanentClips = await invoke("get_uploaded_clips", {
                permanent: true,
            });
            for (const c of permanentClips)
                if (c.thumbnail_path) loadThumb(c.thumbnail_path);
            r2PermBytes = await invoke<number>("r2_storage", {
                permanent: true,
            }).catch(() => 0);
        }
    }

    // ─── Recording controls ────────────────────────────────────────────────

    async function toggleBuffer() {
        try {
            const action = recState.replay_buffer_active
                ? "stop_replay_buffer"
                : "start_replay_buffer";
            await invoke(action);
            notify(
                recState.replay_buffer_active
                    ? "Buffer stopped"
                    : "Buffer started",
            );
        } catch (e: any) {
            notify(String(e), "err");
        }
        await refresh();
    }

    // ─── Sharing ───────────────────────────────────────────────────────────

    function enqueueUpload(clip: Clip, permanent: boolean, event: MouseEvent) {
        if (!event.isTrusted) {
            notify("Upload blocked", "err");
            return;
        }
        if (clip.r2_url) {
            void copyLink(clip);
            return;
        }
        const existing = activeUploadFor(clip.id, permanent);
        if (existing) {
            uploadQueueOpen = true;
            notify(existing.status === "queued" ? "Upload queued" : "Upload running");
            return;
        }

        uploadQueue = [
            ...uploadQueue,
            {
                queueId: `${clip.id}-${permanent ? "p" : "t"}-${Date.now()}`,
                clipId: clip.id,
                filename: clip.filename,
                permanent,
                status: "queued",
                progress: 0,
                message: "queued",
                url: null,
                error: null,
                autoCopy: true,
                createdAt: Date.now(),
            },
        ];
        uploadQueueOpen = true;
        void processUploadQueue();
    }

    async function processUploadQueue() {
        if (uploadWorkerActive) return;
        uploadWorkerActive = true;

        try {
            while (true) {
                const next = uploadQueue
                    .filter((item) => item.status === "queued")
                    .sort((a, b) => a.createdAt - b.createdAt)[0];
                if (!next) break;
                await runUpload(next);
            }
        } finally {
            uploadWorkerActive = false;
        }
    }

    async function runUpload(item: UploadQueueItem) {
        setUploadItem(item.queueId, {
            status: "uploading",
            progress: 8,
            message: "preparing",
            error: null,
        });
        startUploadProgress(item.queueId);

        try {
            const url = await invoke<string>("upload_clip", {
                id: item.clipId,
                permanent: item.permanent,
            });
            stopUploadProgress(item.queueId);
            setUploadItem(item.queueId, {
                status: "uploaded",
                progress: 100,
                message: "uploaded",
                url,
            });

            if (item.autoCopy) {
                await writeText(url);
                notify(item.permanent ? "Permanent link copied" : "Link copied");
            } else {
                notify("Upload complete");
            }

            await refreshUploadViews(item.permanent);
        } catch (e: any) {
            stopUploadProgress(item.queueId);
            setUploadItem(item.queueId, {
                status: "failed",
                progress: 100,
                message: "failed",
                error: String(e || "Upload failed"),
            });
            notify("Upload failed", "err");
        }
    }

    function startUploadProgress(queueId: string) {
        stopUploadProgress(queueId);
        const timer = setInterval(() => {
            const item = uploadQueue.find((u) => u.queueId === queueId);
            if (!item || item.status !== "uploading") {
                stopUploadProgress(queueId);
                return;
            }

            const progress = Math.min(
                92,
                item.progress + (item.progress < 45 ? 5 : item.progress < 75 ? 2 : 1),
            );
            const message =
                progress < 18
                    ? "preparing"
                    : progress < 72
                      ? "uploading video"
                      : progress < 88
                        ? "uploading assets"
                        : "finalizing";
            setUploadItem(queueId, { progress, message });
        }, 350);
        uploadTimers.set(queueId, timer);
    }

    function stopUploadProgress(queueId: string) {
        const timer = uploadTimers.get(queueId);
        if (!timer) return;
        clearInterval(timer);
        uploadTimers.delete(queueId);
    }

    function setUploadItem(
        queueId: string,
        patch: Partial<Omit<UploadQueueItem, "queueId">>,
    ) {
        uploadQueue = uploadQueue.map((item) =>
            item.queueId === queueId ? { ...item, ...patch } : item,
        );
    }

    function activeUploadFor(clipId: string, permanent: boolean) {
        return uploadQueue.find(
            (item) =>
                item.clipId === clipId &&
                item.permanent === permanent &&
                (item.status === "queued" || item.status === "uploading"),
        );
    }

    function uploadButtonLabel(clipId: string, permanent: boolean, fallback: string) {
        const item = activeUploadFor(clipId, permanent);
        if (!item) return fallback;
        if (item.status === "queued") return "queued";
        return `${Math.round(item.progress)}%`;
    }

    function uploadIconLabel(clipId: string, permanent: boolean) {
        const item = activeUploadFor(clipId, permanent);
        if (!item) return "⬆";
        if (item.status === "queued") return "…";
        return `${Math.round(item.progress)}%`;
    }

    function uploadStatusText(item: UploadQueueItem) {
        if (item.status === "uploaded") return item.permanent ? "permanent" : "temporary";
        if (item.status === "failed") return item.error || "failed";
        return item.message;
    }

    function retryUpload(item: UploadQueueItem) {
        setUploadItem(item.queueId, {
            status: "queued",
            progress: 0,
            message: "queued",
            error: null,
            url: null,
            createdAt: Date.now(),
        });
        uploadQueueOpen = true;
        void processUploadQueue();
    }

    function cancelQueuedUpload(item: UploadQueueItem) {
        if (item.status !== "queued") return;
        setUploadItem(item.queueId, {
            status: "canceled",
            progress: 0,
            message: "canceled",
        });
    }

    async function copyQueuedLink(item: UploadQueueItem) {
        if (!item.url) return;
        await writeText(item.url);
        notify("Link copied");
    }

    function dismissUpload(queueId: string) {
        stopUploadProgress(queueId);
        uploadQueue = uploadQueue.filter((item) => item.queueId !== queueId);
    }

    function clearSettledUploads() {
        for (const item of uploadQueue) {
            if (item.status !== "queued" && item.status !== "uploading") {
                stopUploadProgress(item.queueId);
            }
        }
        uploadQueue = uploadQueue.filter(
            (item) => item.status === "queued" || item.status === "uploading",
        );
    }

    async function refreshUploadViews(permanent: boolean) {
        await loadClips();
        if (tab === "uploads" && !permanent) await switchTab("uploads");
        if (tab === "permanent" && permanent) await switchTab("permanent");
    }

    const quickShare = (c: Clip, e: MouseEvent) => enqueueUpload(c, false, e);
    const permUpload = (c: Clip, e: MouseEvent) => enqueueUpload(c, true, e);

    async function copyLink(clip: Clip) {
        if (!clip.r2_url) return;
        await writeText(clip.r2_url);
        notify("Link copied");
    }

    async function copyPath(clip: Clip) {
        await writeText(clip.path);
        notify("Path copied");
    }

    async function revealClip(clip: Clip) {
        try {
            await revealItemInDir(clip.path);
        } catch {
            notify("Reveal failed", "err");
        }
    }

    async function toggleFavorite(clip: Clip) {
        const favorite = !clip.favorite;
        try {
            await invoke("update_clip_favorite", { id: clip.id, favorite });
            clips = clips.map((c) => (c.id === clip.id ? { ...c, favorite } : c));
            uploadedClips = uploadedClips.map((c) =>
                c.id === clip.id ? { ...c, favorite } : c,
            );
            permanentClips = permanentClips.map((c) =>
                c.id === clip.id ? { ...c, favorite } : c,
            );
            notify(favorite ? "Pinned" : "Unpinned");
        } catch {
            notify("Favorite failed", "err");
        }
    }

    function resetLibraryControls() {
        librarySearch = "";
        libraryFilter = "all";
        librarySort = "newest";
    }

    function selectedLabel(options: SelectOption[], value: string | undefined) {
        return options.find((option) => option.value === value)?.label ?? value ?? "";
    }

    function toggleDropdown(id: string) {
        openDropdown = openDropdown === id ? null : id;
    }

    function closeDropdown() {
        openDropdown = null;
    }

    function handleDropdownKey(
        e: KeyboardEvent,
        id: string,
        options: SelectOption[],
        value: string,
        setValue: (value: string) => void,
    ) {
        if (e.key === "Escape") {
            closeDropdown();
            return;
        }
        if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleDropdown(id);
            return;
        }
        if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;

        e.preventDefault();
        const current = Math.max(
            0,
            options.findIndex((option) => option.value === value),
        );
        const delta = e.key === "ArrowDown" ? 1 : -1;
        const next = (current + delta + options.length) % options.length;
        setValue(options[next].value);
    }

    async function deleteClip(clip: Clip) {
        if (clip.r2_url) {
            try {
                await invoke("delete_from_r2", { id: clip.id });
            } catch (e) {
                notify("R2 delete failed", "err");
                return;
            }
        }
        await invoke("delete_clip", { id: clip.id });
        notify("Deleted");
        await loadClips();
        if (tab === "uploads" || tab === "permanent") {
            await refreshR2TabState();
        }
    }

    async function renameClip(clip: Clip) {
        const stem = clip.filename.replace(/\.[^.]+$/, "");
        const newName = prompt("Rename clip:", stem);
        if (!newName || newName === stem) return;
        try {
            await invoke("rename_clip", { id: clip.id, newName });
            notify("Renamed");
            await loadClips();
        } catch (e: any) {
            notify("Rename failed", "err");
        }
    }

    async function deleteFromR2(clip: Clip) {
        await invoke("delete_from_r2", { id: clip.id });
        notify("Removed from R2");
        await switchTab(tab);
    }

    // ─── Editor ────────────────────────────────────────────────────────────

    async function openEditor(clip: Clip) {
        editClip = clip;
        trimStart = 0;
        trimEnd = clip.duration || 10;
        tab = "editor";
        previewSrc = "";
        editorAudioTracks = [];
        editorMutedAudioTracks = {};

        try {
            editorAudioTracks = await invoke<AudioTrack[]>("get_clip_audio_tracks", {
                path: clip.path,
            });
            await loadEditorPreview();
        } catch (e) {
            console.error(e);
            notify("Preview failed", "err");
        }
    }

    function closeEditor() {
        previewSrc = "";
        editClip = null;
        editorAudioTracks = [];
        editorMutedAudioTracks = {};
        tab = "library";
    }

    function enabledEditorAudioTracks() {
        if (editorAudioTracks.length === 0) return null;
        return editorAudioTracks
            .filter((track) => !editorMutedAudioTracks[track.index])
            .map((track) => track.index);
    }

    async function loadEditorPreview() {
        if (!editClip) return;
        previewLoading = true;
        previewSrc = "";

        try {
            const path = await invoke<string>("transcode_for_preview", {
                input: editClip.path,
                enabledAudioTracks: enabledEditorAudioTracks(),
            });
            previewSrc = await invoke<string>("serve_video", { path });
        } catch (e) {
            console.error(e);
            notify("Preview failed", "err");
        } finally {
            previewLoading = false;
        }
    }

    function toggleEditorAudioTrack(track: AudioTrack) {
        editorMutedAudioTracks = {
            ...editorMutedAudioTracks,
            [track.index]: !editorMutedAudioTracks[track.index],
        };
        void loadEditorPreview();
    }

    async function exportTrim(mode: "copy" | "overwrite" = "copy") {
        if (!editClip) return;

        const ext = editClip.path.split(".").pop() || "mp4";
        const orig = editClip.path;
        const tmp = orig.replace(/\.[^.]+$/, `_tmp_${Date.now()}.${ext}`);
        const final_path =
            mode === "overwrite"
                ? orig
                : orig.replace(/\.[^.]+$/, `_trimmed.${ext}`);

        try {
            await invoke("trim_clip", {
                input: orig,
                output: tmp,
                start: trimStart,
                end: trimEnd,
                enabledAudioTracks: enabledEditorAudioTracks(),
            });
            await invoke("replace_file", { src: tmp, dst: final_path });
            notify(mode === "overwrite" ? "Clip overwritten" : "Saved as copy");
            closeEditor();
            setTimeout(loadClips, 500);
        } catch (e) {
            console.error(e);
            notify("Export failed", "err");
        }
    }

    // ─── Settings ──────────────────────────────────────────────────────────

    async function saveSettings() {
        await invoke("save_settings", { newSettings: settings });
        notify("Settings saved");
    }

    async function loadAudioInputDevices() {
        audioInputDevicesLoading = true;
        try {
            const devices = await invoke<SelectOption[]>("list_audio_input_devices");
            audioInputOptions = devices.length
                ? devices
                : [{ value: "default_input", label: "Default input" }];
        } catch (e) {
            console.error(e);
            audioInputOptions = [{ value: "default_input", label: "Default input" }];
            notify("Mic list failed", "err");
        } finally {
            audioInputDevicesLoading = false;
        }
    }

    function updateAudioSetting(key: string, value: string | boolean) {
        settings = { ...settings, [key]: value };
        void invoke("save_settings", { newSettings: settings }).catch((e) => {
            console.error(e);
            notify("Audio setting save failed", "err");
        });
    }

    function updateAudioToggle(key: string, value: boolean) {
        updateAudioSetting(key, value);
    }

    async function previewClippingSound() {
        try {
            await invoke("save_settings", { newSettings: settings });
            await invoke("play_clip_sound");
        } catch (e) {
            console.error(e);
            notify(String(e), "err");
        }
    }

    // ─── Theme ─────────────────────────────────────────────────────────────

    function setThemeVar(key: keyof ThemeVars, value: string) {
        theme.updateVar(key, value);
        void persistTheme();
    }

    function resetTheme() {
        theme.reset();
        void persistTheme();
        void applyWindowOpacity(theme.current()["--bg-opacity"]);
        notify("Theme reset to defaults");
    }

    async function exportTheme() {
        const css = theme.exportCSS();
        await writeText(css);
        notify("Theme CSS copied to clipboard");
    }

    function importThemeFromFile() {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = ".css";
        input.onchange = () => {
            const file = input.files?.[0];
            if (!file) return;
            const reader = new FileReader();
                reader.onload = () => {
                    theme.importCSS(reader.result as string);
                    void persistTheme();
                    void applyWindowOpacity(theme.current()["--bg-opacity"]);
                    notify("Theme imported");
                };
            reader.readAsText(file);
        };
        input.click();
    }

    // ─── Formatters ────────────────────────────────────────────────────────

    function fmt(seconds: number) {
        if (!seconds || isNaN(seconds)) return "0:00";
        const m = Math.floor(seconds / 60);
        const s = Math.floor(seconds % 60)
            .toString()
            .padStart(2, "0");
        return `${m}:${s}`;
    }

    function timeAgo(iso: string) {
        const s = (Date.now() - new Date(iso).getTime()) / 1000;
        if (s < 60) return "just now";
        if (s < 3600) return `${Math.floor(s / 60)}m ago`;
        if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
        return `${Math.floor(s / 86400)}d ago`;
    }

    function fmtBytes(b: number) {
        return b < 1e9
            ? `${(b / 1e6).toFixed(0)} MB`
            : `${(b / 1e9).toFixed(1)} GB`;
    }
</script>

{#if toast}
    <div class="toast" class:err={toast.kind === "err"}>
        <span class="toast-prefix">{toast.kind === "err" ? "✗" : "→"}</span
        >{toast.msg}
    </div>
{/if}

{#if uploadQueue.length > 0}
    <aside class="upload-dock" class:closed={!uploadQueueOpen}>
        <button
            class="upload-dock-head"
            onclick={() => (uploadQueueOpen = !uploadQueueOpen)}
            title={uploadQueueOpen ? "Hide upload queue" : "Show upload queue"}
        >
            <span>uploads</span>
            <span class="upload-dock-meta">
                {#if activeUploadCount > 0}{activeUploadCount} active{/if}
                {#if activeUploadCount > 0 && failedUploadCount > 0} / {/if}
                {#if failedUploadCount > 0}{failedUploadCount} failed{/if}
                {#if activeUploadCount === 0 && failedUploadCount === 0}
                    {uploadQueue.length} done
                {/if}
            </span>
            <span class="upload-dock-toggle">{uploadQueueOpen ? "▾" : "▴"}</span>
        </button>

        {#if uploadQueueOpen}
            <div class="upload-queue">
                {#each uploadQueue as item (item.queueId)}
                    <div class="upload-item" class:failed={item.status === "failed"}>
                        <div class="upload-row">
                            <div class="upload-main">
                                <span class="upload-name" title={item.filename}
                                    >{item.filename}</span
                                >
                                <span class="upload-sub"
                                    >{item.permanent ? "permanent" : "temporary"} · {uploadStatusText(
                                        item,
                                    )}</span
                                >
                            </div>
                            <div class="upload-actions">
                                {#if item.status === "uploaded"}
                                    <button
                                        class="queue-btn"
                                        onclick={() => copyQueuedLink(item)}>copy</button
                                    >
                                    <button
                                        class="queue-btn"
                                        onclick={() => dismissUpload(item.queueId)}
                                        >clear</button
                                    >
                                {:else if item.status === "failed"}
                                    <button
                                        class="queue-btn"
                                        onclick={() => retryUpload(item)}>retry</button
                                    >
                                    <button
                                        class="queue-btn"
                                        onclick={() => dismissUpload(item.queueId)}
                                        >clear</button
                                    >
                                {:else if item.status === "queued"}
                                    <button
                                        class="queue-btn"
                                        onclick={() => cancelQueuedUpload(item)}
                                        >cancel</button
                                    >
                                {:else if item.status === "canceled"}
                                    <button
                                        class="queue-btn"
                                        onclick={() => dismissUpload(item.queueId)}
                                        >clear</button
                                    >
                                {/if}
                            </div>
                        </div>
                        <div class="upload-progress" aria-label={item.message}>
                            <div
                                class="upload-progress-fill"
                                style="width: {item.progress}%"
                            ></div>
                        </div>
                    </div>
                {/each}
                {#if settledUploadCount > 0}
                    <button class="clear-uploads" onclick={clearSettledUploads}>
                        clear inactive
                    </button>
                {/if}
            </div>
        {/if}
    </aside>
{/if}

{#if bootPhase !== "ready"}
    <section class="boot-screen" aria-label="Loading Klyppd">
        <div class="boot-terminal" aria-label="Boot terminal">
            <div class="boot-terminal-head">
                <span>klyppd tty1</span>
                <span>{Math.round(bootProgress)}%</span>
            </div>
            <div class="boot-terminal-lines">
                {#each bootTerminalLines as line}
                    <div>{line}</div>
                {/each}
                <span class="boot-cursor" aria-hidden="true">_</span>
            </div>
            <div class="boot-progress-label">
                <span>loading klyppd</span>
                <span>{Math.round(bootProgress)}%</span>
            </div>
            <div class="boot-bar" aria-hidden="true">
                <div class="boot-bar-fill" style="width: {bootProgress}%"></div>
            </div>
        </div>
    </section>
{/if}

<div class="app">
    <header data-tauri-drag-region>
        <div class="brand">
            <img src="/logo.png" alt="klyppd" />
            <span class="brand-sep">│</span>
        </div>
        <div class="header-info">
            <button
                class="rec-btn"
                class:on={recState.replay_buffer_active}
                onclick={toggleBuffer}
            >
                <span class="rec-indicator"
                    >{recState.replay_buffer_active ? "●" : "○"}</span
                >
                {recState.replay_buffer_active ? "rec" : "idle"}
            </button>
            <span class="info-tag">{settings.codec || "h264"}</span>
            <span class="info-tag">{settings.fps || 60}fps</span>
            <span class="info-tag">{fmtBytes(storageUsage)}</span>
        </div>
        <div class="window-ctrls">
            <button
                onclick={() => getCurrentWindow().hide()}
                aria-label="hide"
                class="wc-min"
                title="Hide">─</button
            >
            <button
                onclick={() => getCurrentWindow().close()}
                aria-label="close"
                class="wc-close">✕</button
            >
        </div>
    </header>

    <div class="layout">
        <nav class="sidenav" class:collapsed={sidebarCollapsed}>
            <button
                class="collapse-toggle"
                onclick={() => (sidebarCollapsed = !sidebarCollapsed)}
                title={sidebarCollapsed ? "Expand" : "Collapse"}
            >
                {sidebarCollapsed ? "›" : "‹"}
            </button>
            <div class="nav-items">
                <button
                    class:sel={tab === "library"}
                    onclick={() => switchTab("library")}
                    title="Library"
                >
                    <svg
                        class="nav-icon"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        
                        ><rect x="3" y="3" width="7" height="7" rx="1" /><rect
                            x="14"
                            y="3"
                            width="7"
                            height="7"
                            rx="1"
                        /><rect x="3" y="14" width="7" height="7" rx="1" /><rect
                            x="14"
                            y="14"
                            width="7"
                            height="7"
                            rx="1"
                        /></svg
                    >
                    {#if !sidebarCollapsed}<span class="nav-label">library</span
                        >{/if}
                </button>
                <button
                    class:sel={tab === "uploads"}
                    onclick={() => switchTab("uploads")}
                    title="Uploads"
                >
                    <svg
                        class="nav-icon"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        
                        ><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" /></svg
                    >
                    {#if !sidebarCollapsed}<span class="nav-label">uploads</span
                        >{/if}
                </button>
                <button
                    class:sel={tab === "permanent"}
                    onclick={() => switchTab("permanent")}
                    title="Permanent"
                >
                    <svg
                        class="nav-icon"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        
                        ><path d="M19 21l-7-5-7 5V5a2 2 0 012-2h10a2 2 0 012 2z" /></svg
                    >
                    {#if !sidebarCollapsed}<span class="nav-label"
                            >permanent</span
                        >{/if}
                </button>
            </div>
            <div class="nav-spacer"></div>
            <button
                class:sel={tab === "settings"}
                onclick={() => switchTab("settings")}
                title="Settings"
            >
                <svg
                    class="nav-icon"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    
                    ><line x1="4" y1="21" x2="4" y2="14" /><line x1="4" y1="10" x2="4" y2="3" /><line x1="12" y1="21" x2="12" y2="12" /><line x1="12" y1="8" x2="12" y2="3" /><line x1="20" y1="21" x2="20" y2="16" /><line x1="20" y1="12" x2="20" y2="3" /><line x1="1" y1="14" x2="7" y2="14" /><line x1="9" y1="8" x2="15" y2="8" /><line x1="17" y1="16" x2="23" y2="16" /></svg
                >
                {#if !sidebarCollapsed}<span class="nav-label">settings</span
                    >{/if}
            </button>
        </nav>

        <main>
            {#if tab === "library"}
                <div class="page-head">
                    <div class="page-title">
                        <span class="prompt">~</span>
                        <h1>library</h1>
                        <span class="count">{filteredClips.length}/{clips.length}</span>
                    </div>
                    <div class="page-controls">
                        <input
                            class="library-search"
                            bind:value={librarySearch}
                            placeholder="search"
                            aria-label="Search clips"
                        />
                        <div class="terminal-select" class:open={openDropdown === "library-filter"}>
                            <button
                                type="button"
                                class="terminal-select-btn"
                                aria-haspopup="listbox"
                                aria-expanded={openDropdown === "library-filter"}
                                onclick={() => toggleDropdown("library-filter")}
                                onkeydown={(e) =>
                                    handleDropdownKey(
                                        e,
                                        "library-filter",
                                        libraryFilterOptions,
                                        libraryFilter,
                                        (value) => (libraryFilter = value as LibraryFilter),
                                    )}
                            >
                                <span>{selectedLabel(libraryFilterOptions, libraryFilter)}</span>
                                <span class="terminal-caret">▾</span>
                            </button>
                            {#if openDropdown === "library-filter"}
                                <div class="terminal-menu" role="listbox">
                                    {#each libraryFilterOptions as option}
                                        <button
                                            type="button"
                                            class:sel={libraryFilter === option.value}
                                            role="option"
                                            aria-selected={libraryFilter === option.value}
                                            onclick={() => {
                                                libraryFilter = option.value;
                                                closeDropdown();
                                            }}
                                        >
                                            <span class="terminal-check"
                                                >{libraryFilter === option.value ? ">" : ""}</span
                                            >
                                            {option.label}
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                        <div class="terminal-select" class:open={openDropdown === "library-sort"}>
                            <button
                                type="button"
                                class="terminal-select-btn"
                                aria-haspopup="listbox"
                                aria-expanded={openDropdown === "library-sort"}
                                onclick={() => toggleDropdown("library-sort")}
                                onkeydown={(e) =>
                                    handleDropdownKey(
                                        e,
                                        "library-sort",
                                        librarySortOptions,
                                        librarySort,
                                        (value) => (librarySort = value as LibrarySort),
                                    )}
                            >
                                <span>{selectedLabel(librarySortOptions, librarySort)}</span>
                                <span class="terminal-caret">▾</span>
                            </button>
                            {#if openDropdown === "library-sort"}
                                <div class="terminal-menu" role="listbox">
                                    {#each librarySortOptions as option}
                                        <button
                                            type="button"
                                            class:sel={librarySort === option.value}
                                            role="option"
                                            aria-selected={librarySort === option.value}
                                            onclick={() => {
                                                librarySort = option.value;
                                                closeDropdown();
                                            }}
                                        >
                                            <span class="terminal-check"
                                                >{librarySort === option.value ? ">" : ""}</span
                                            >
                                            {option.label}
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                        {#if librarySearch || libraryFilter !== "all" || librarySort !== "newest"}
                            <button
                                class="view-toggle"
                                onclick={resetLibraryControls}
                                title="Clear filters">×</button
                            >
                        {/if}
                        <button
                            class="view-toggle"
                            class:active={viewMode === "grid"}
                            onclick={() => {
                                viewMode = "grid";
                                forceRepaint();
                            }}
                            title="Grid view">▦</button
                        >
                        <button
                            class="view-toggle"
                            class:active={viewMode === "list"}
                            onclick={() => {
                                viewMode = "list";
                                forceRepaint();
                            }}
                            title="List view">☰</button
                        >
                    </div>
                </div>
                <div class="separator"></div>
                {#if clips.length === 0}
                    <div class="empty">
                        <pre class="empty-art">  ╭─────────────────╮
  │  no clips yet    │
  ╰─────────────────╯</pre>
                        <p>
                            press <kbd
                                >{settings.hotkey_save_replay || "Alt+R"}</kbd
                            > while buffer is running
                        </p>
                    </div>
                {:else if filteredClips.length === 0}
                    <div class="empty">
                        <pre class="empty-art">  ╭─────────────────╮
  │  no matches     │
  ╰─────────────────╯</pre>
                        <p>clear filters or try another search</p>
                    </div>
                {:else if viewMode === "grid"}
                    <div class="grid">
                        {#each filteredClips as clip}
                            <article class="clip-card" class:fav={clip.favorite}>
                                <div
                                    class="clip-thumb"
                                    role="button"
                                    tabindex="0"
                                    onclick={() => openEditor(clip)}
                                    onkeydown={(e) =>
                                        e.key === "Enter" && openEditor(clip)}
                                >
                                    {#if clip.thumbnail_path && thumbCache[clip.thumbnail_path]}
                                        <img
                                            src={thumbCache[
                                                clip.thumbnail_path
                                            ]}
                                            alt=""
                                        />
                                    {:else}
                                        <div class="thumb-placeholder">▶</div>
                                    {/if}
                                    <span class="clip-dur"
                                        >{fmt(clip.duration)}</span
                                    >
                                    <div class="clip-hover">
                                        <span>edit</span>
                                    </div>
                                </div>
                                <div class="clip-info">
                                    <span
                                        class="clip-name"
                                        title={clip.filename}
                                        ondblclick={() => renameClip(clip)}
                                        >{clip.filename}</span
                                    >
                                    <span class="clip-time"
                                        >{timeAgo(clip.created_at)}</span
                                    >
                                </div>
                                <div class="clip-actions">
                                    <button
                                        class="act-sec pin"
                                        class:on={clip.favorite}
                                        onclick={() => toggleFavorite(clip)}
                                        title={clip.favorite ? "unpin" : "pin"}
                                        >{clip.favorite ? "◆" : "◇"}</button
                                    >
                                    {#if clip.r2_url}
                                        <button
                                            class="act-primary"
                                            onclick={() => copyLink(clip)}
                                            title={clip.is_permanent
                                                ? "permanent"
                                                : "temporary"}
                                        >
                                            copy link
                                        </button>
                                    {:else}
                                        <button
                                            class="act-primary"
                                            onclick={(e) => quickShare(clip, e)}
                                            >{uploadButtonLabel(
                                                clip.id,
                                                false,
                                                "share",
                                            )}</button
                                        >
                                        <button
                                            class="act-sec"
                                            onclick={(e) => permUpload(clip, e)}
                                            title="permanent upload"
                                            >{uploadIconLabel(clip.id, true)}</button
                                        >
                                    {/if}
                                    <button
                                        class="act-sec"
                                        onclick={() => deleteClip(clip)}
                                        title="delete">✕</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={() => revealClip(clip)}
                                        title="reveal">↗</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={() => copyPath(clip)}
                                        title="copy path">⎘</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={() => renameClip(clip)}
                                        title="rename">✎</button
                                    >
                                </div>
                            </article>
                        {/each}
                    </div>
                {:else}
                    <div class="list">
                        {#each filteredClips as clip}
                            <div
                                class="list-row"
                                class:fav={clip.favorite}
                                role="button"
                                tabindex="0"
                                onclick={() => openEditor(clip)}
                                onkeydown={(e) =>
                                    e.key === "Enter" && openEditor(clip)}
                            >
                                <div class="list-thumb">
                                    {#if clip.thumbnail_path && thumbCache[clip.thumbnail_path]}
                                        <img
                                            src={thumbCache[
                                                clip.thumbnail_path
                                            ]}
                                            alt=""
                                        />
                                    {:else}
                                        <span class="list-thumb-ph">▶</span>
                                    {/if}
                                </div>
                                <div class="list-meta">
                                    <span
                                        class="list-name"
                                        ondblclick={(e) => {
                                            e.stopPropagation();
                                            renameClip(clip);
                                        }}>{clip.filename}</span
                                    >
                                    <span class="list-sub"
                                        >{fmt(clip.duration)} · {timeAgo(
                                            clip.created_at,
                                        )}</span
                                    >
                                </div>
                                <div
                                    class="list-actions"
                                    onclick={(e) => e.stopPropagation()}
                                >
                                    <button
                                        class="act-sec pin"
                                        class:on={clip.favorite}
                                        onclick={() => toggleFavorite(clip)}
                                        title={clip.favorite ? "unpin" : "pin"}
                                        >{clip.favorite ? "◆" : "◇"}</button
                                    >
                                    {#if clip.r2_url}
                                        <button
                                            class="act-primary"
                                            onclick={() => copyLink(clip)}
                                            >copy</button
                                        >
                                    {:else}
                                        <button
                                            class="act-primary"
                                            onclick={(e) => quickShare(clip, e)}
                                            >{uploadButtonLabel(
                                                clip.id,
                                                false,
                                                "share",
                                            )}</button
                                        >
                                        <button
                                            class="act-sec"
                                            onclick={(e) => permUpload(clip, e)}
                                            >{uploadIconLabel(clip.id, true)}</button
                                        >
                                    {/if}
                                    <button
                                        class="act-sec del"
                                        onclick={() => deleteClip(clip)}
                                        >✕</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={() => revealClip(clip)}
                                        title="reveal">↗</button
                                    >
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}
            {/if}

            {#if tab === "uploads"}
                <div class="page-head">
                    <div class="page-title">
                        <span class="prompt">~</span>
                        <h1>temporary uploads</h1>
                        <span class="count">{uploadedClips.length}</span>
                    </div>
                    <span class="page-stat">{fmtBytes(r2TempBytes)} on r2</span>
                </div>
                <div class="separator"></div>
                {#if uploadedClips.length === 0}
                    <div class="empty">
                        <pre class="empty-art">  ╭──────────────────────╮
  │  no temp uploads     │
  ╰──────────────────────╯</pre>
                        <p>use the share button to upload clips</p>
                    </div>
                {:else}
                    <div class="grid">
                        {#each uploadedClips as clip}
                            <article class="clip-card" class:fav={clip.favorite}>
                                <div
                                    class="clip-thumb"
                                    role="button"
                                    tabindex="0"
                                    onclick={() => openEditor(clip)}
                                    onkeydown={(e) =>
                                        e.key === "Enter" && openEditor(clip)}
                                >
                                    {#if clip.thumbnail_path && thumbCache[clip.thumbnail_path]}
                                        <img
                                            src={thumbCache[
                                                clip.thumbnail_path
                                            ]}
                                            alt=""
                                        />
                                    {:else}
                                        <div class="thumb-placeholder">▶</div>
                                    {/if}
                                    <span class="clip-dur"
                                        >{fmt(clip.duration)}</span
                                    >
                                </div>
                                <div class="clip-info">
                                    <span
                                        class="clip-name"
                                        title={clip.filename}
                                        ondblclick={() => renameClip(clip)}
                                        >{clip.filename}</span
                                    >
                                    <span class="clip-time"
                                        >{#if clip.expiry_date}expires {new Date(
                                                clip.expiry_date,
                                            ).toLocaleDateString()}{/if}</span
                                    >
                                </div>
                                <div class="clip-actions">
                                    <button
                                        class="act-sec pin"
                                        class:on={clip.favorite}
                                        onclick={() => toggleFavorite(clip)}
                                        title={clip.favorite ? "unpin" : "pin"}
                                        >{clip.favorite ? "◆" : "◇"}</button
                                    >
                                    <button
                                        class="act-primary"
                                        onclick={() => copyLink(clip)}
                                        >copy link</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={() => revealClip(clip)}
                                        title="reveal">↗</button
                                    >
                                    <button
                                        class="act-sec del"
                                        onclick={() => deleteFromR2(clip)}
                                        >delete</button
                                    >
                                </div>
                            </article>
                        {/each}
                    </div>
                {/if}
            {/if}

            {#if tab === "permanent"}
                <div class="page-head">
                    <div class="page-title">
                        <span class="prompt">~</span>
                        <h1>permanent uploads</h1>
                        <span class="count">{permanentClips.length}</span>
                    </div>
                    <span class="page-stat">{fmtBytes(r2PermBytes)} on r2</span>
                </div>
                <div class="separator"></div>
                {#if permanentClips.length === 0}
                    <div class="empty">
                        <pre class="empty-art">  ╭──────────────────────╮
  │  no perm uploads     │
  ╰──────────────────────╯</pre>
                    </div>
                {:else}
                    <div class="grid">
                        {#each permanentClips as clip}
                            <article class="clip-card" class:fav={clip.favorite}>
                                <div
                                    class="clip-thumb"
                                    role="button"
                                    tabindex="0"
                                    onclick={() => openEditor(clip)}
                                    onkeydown={(e) =>
                                        e.key === "Enter" && openEditor(clip)}
                                >
                                    {#if clip.thumbnail_path && thumbCache[clip.thumbnail_path]}
                                        <img
                                            src={thumbCache[
                                                clip.thumbnail_path
                                            ]}
                                            alt=""
                                        />
                                    {:else}
                                        <div class="thumb-placeholder">▶</div>
                                    {/if}
                                    <span class="clip-dur"
                                        >{fmt(clip.duration)}</span
                                    >
                                </div>
                                <div class="clip-info">
                                    <span
                                        class="clip-name"
                                        title={clip.filename}
                                        ondblclick={() => renameClip(clip)}
                                        >{clip.filename}</span
                                    >
                                    <span class="clip-time">permanent</span>
                                </div>
                                <div class="clip-actions">
                                    <button
                                        class="act-sec pin"
                                        class:on={clip.favorite}
                                        onclick={() => toggleFavorite(clip)}
                                        title={clip.favorite ? "unpin" : "pin"}
                                        >{clip.favorite ? "◆" : "◇"}</button
                                    >
                                    <button
                                        class="act-primary"
                                        onclick={() => copyLink(clip)}
                                        >copy link</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={() => revealClip(clip)}
                                        title="reveal">↗</button
                                    >
                                    <button
                                        class="act-sec del"
                                        onclick={() => deleteFromR2(clip)}
                                        >delete from r2</button
                                    >
                                </div>
                            </article>
                        {/each}
                    </div>
                {/if}
            {/if}

            {#if tab === "editor" && editClip}
                <div class="editor">
                    <div class="editor-bar">
                        <button class="ed-back" onclick={closeEditor}
                            >← back</button
                        >
                        <span class="ed-filename">{editClip.filename}</span>
                        <div class="ed-spacer"></div>
                        <button
                            class="act-sec"
                            onclick={() => exportTrim("overwrite")}
                            disabled={!previewSrc}>overwrite</button
                        >
                        <button
                            class="act-primary"
                            onclick={() => exportTrim("copy")}
                            disabled={!previewSrc}
                        >
                            save copy ({fmt(trimEnd - trimStart)})
                        </button>
                    </div>

                    {#if editorAudioTracks.length > 0}
                        <div class="audio-track-strip">
                            {#each editorAudioTracks as track}
                                <button
                                    type="button"
                                    class="audio-track-toggle"
                                    class:muted={editorMutedAudioTracks[track.index]}
                                    class:mixed={!track.isolated}
                                    onclick={() => toggleEditorAudioTrack(track)}
                                    title={track.isolated ? track.label : `${track.label} may include app audio`}
                                >
                                    <span>{editorMutedAudioTracks[track.index] ? "off" : "on"}</span>
                                    {track.label}
                                </button>
                            {/each}
                        </div>
                    {/if}

                    <div class="video-wrap">
                        {#if previewLoading}
                            <div class="vid-loading">
                                <span class="spinner"></span>preparing preview…
                            </div>
                        {:else if previewSrc}
                            <video
                                bind:this={videoEl}
                                src={previewSrc}
                                controls
                                preload="auto"
                                ontimeupdate={() => {
                                    if (videoEl)
                                        currentTime = videoEl.currentTime;
                                }}
                                onloadedmetadata={() => {
                                    if (videoEl) {
                                        duration = videoEl.duration;
                                        if (!trimEnd || trimEnd <= 0)
                                            trimEnd = duration;
                                    }
                                }}
                            ></video>
                        {/if}
                    </div>

                    <div class="tl-section">
                        <div class="tl-readout">
                            <span>{fmt(currentTime)}/{fmt(duration)}</span>
                            <span class="tl-trim"
                                >[{fmt(trimStart)} → {fmt(trimEnd)}]</span
                            >
                        </div>
                        <!-- svelte-ignore a11y_click_events_have_key_events -->
                        <!-- svelte-ignore a11y_no_static_element_interactions -->
                        <div
                            class="tl"
                            onclick={(e) => {
                                const rect = (
                                    e.currentTarget as HTMLElement
                                ).getBoundingClientRect();
                                const pct = Math.max(
                                    0,
                                    Math.min(
                                        1,
                                        (e.clientX - rect.left) / rect.width,
                                    ),
                                );
                                if (videoEl)
                                    videoEl.currentTime = pct * duration;
                            }}
                        >
                            <div class="tl-track"></div>
                            <div
                                class="tl-region"
                                style="left:{(trimStart / (duration || 1)) *
                                    100}%;width:{((trimEnd - trimStart) /
                                    (duration || 1)) *
                                    100}%"
                            ></div>
                            <!-- svelte-ignore a11y_no_static_element_interactions -->
                            <div
                                class="tl-handle"
                                style="left:{(trimStart / (duration || 1)) *
                                    100}%"
                                onmousedown={(e) => {
                                    e.stopPropagation();
                                    const rect = (
                                        e.currentTarget
                                            .parentElement as HTMLElement
                                    ).getBoundingClientRect();
                                    const move = (ev: MouseEvent) => {
                                        const pct = Math.max(
                                            0,
                                            Math.min(
                                                1,
                                                (ev.clientX - rect.left) /
                                                    rect.width,
                                            ),
                                        );
                                        trimStart = Math.min(
                                            pct * duration,
                                            trimEnd - 0.5,
                                        );
                                        if (videoEl) videoEl.currentTime = trimStart;
                                    };
                                    const up = () => {
                                        document.removeEventListener(
                                            "mousemove",
                                            move,
                                        );
                                        document.removeEventListener(
                                            "mouseup",
                                            up,
                                        );
                                    };
                                    document.addEventListener(
                                        "mousemove",
                                        move,
                                    );
                                    document.addEventListener("mouseup", up);
                                }}
                            ></div>
                            <!-- svelte-ignore a11y_no_static_element_interactions -->
                            <div
                                class="tl-handle"
                                style="left:{(trimEnd / (duration || 1)) *
                                    100}%"
                                onmousedown={(e) => {
                                    e.stopPropagation();
                                    const rect = (
                                        e.currentTarget
                                            .parentElement as HTMLElement
                                    ).getBoundingClientRect();
                                    const move = (ev: MouseEvent) => {
                                        const pct = Math.max(
                                            0,
                                            Math.min(
                                                1,
                                                (ev.clientX - rect.left) /
                                                    rect.width,
                                            ),
                                        );
                                        trimEnd = Math.max(
                                            pct * duration,
                                            trimStart + 0.5,
                                        );
                                        if (videoEl) videoEl.currentTime = trimEnd;
                                    };
                                    const up = () => {
                                        document.removeEventListener(
                                            "mousemove",
                                            move,
                                        );
                                        document.removeEventListener(
                                            "mouseup",
                                            up,
                                        );
                                    };
                                    document.addEventListener(
                                        "mousemove",
                                        move,
                                    );
                                    document.addEventListener("mouseup", up);
                                }}
                            ></div>
                            <div
                                class="tl-cursor"
                                style="left:{(currentTime / (duration || 1)) *
                                    100}%"
                            ></div>
                        </div>
                        <div class="tl-btns">
                            <button
                                class="act-sec"
                                onclick={() => (trimStart = currentTime)}
                                >set in ←</button
                            >
                            <button
                                class="act-sec"
                                onclick={() => (trimEnd = currentTime)}
                                >→ set out</button
                            >
                        </div>
                    </div>
                </div>
            {/if}

            {#if tab === "settings"}
                <div class="page-head">
                    <div class="page-title">
                        <span class="prompt">~</span>
                        <h1>settings</h1>
                    </div>
                </div>
                <div class="separator"></div>
                <div class="settings">
                    <section class="cfg-section">
                        <h3>── recording</h3>
                        <div class="cfg">
                            <label>clips_directory</label><input
                                bind:value={settings.clips_directory}
                            />
                        </div>
                        <div class="cfg">
                            <label>buffer_seconds</label>
                            <div class="cfg-suffix">
                                <input
                                    type="number"
                                    bind:value={settings.buffer_seconds}
                                /><span>s</span>
                            </div>
                        </div>
                        <div class="cfg">
                            <label>fps</label>
                            <div class="cfg-suffix">
                                <input
                                    type="number"
                                    bind:value={settings.fps}
                                /><span>fps</span>
                            </div>
                        </div>
                        <div class="cfg">
                            <label>codec</label>
                            <div class="terminal-select cfg-select" class:open={openDropdown === "settings-codec"}>
                                <button
                                    type="button"
                                    class="terminal-select-btn"
                                    aria-haspopup="listbox"
                                    aria-expanded={openDropdown === "settings-codec"}
                                    onclick={() => toggleDropdown("settings-codec")}
                                    onkeydown={(e) =>
                                        handleDropdownKey(
                                            e,
                                            "settings-codec",
                                            codecOptions,
                                            settings.codec || "h264",
                                            (value) => (settings.codec = value),
                                        )}
                                >
                                    <span>{selectedLabel(codecOptions, settings.codec || "h264")}</span>
                                    <span class="terminal-caret">▾</span>
                                </button>
                                {#if openDropdown === "settings-codec"}
                                    <div class="terminal-menu" role="listbox">
                                        {#each codecOptions as option}
                                            <button
                                                type="button"
                                                class:sel={(settings.codec || "h264") === option.value}
                                                role="option"
                                                aria-selected={(settings.codec || "h264") === option.value}
                                                onclick={() => {
                                                    settings.codec = option.value;
                                                    closeDropdown();
                                                }}
                                            >
                                                <span class="terminal-check"
                                                    >{(settings.codec || "h264") === option.value
                                                        ? ">"
                                                        : ""}</span
                                                >
                                                {option.label}
                                            </button>
                                        {/each}
                                    </div>
                                {/if}
                            </div>
                        </div>
                        <div class="cfg">
                            <label>container</label>
                            <div class="terminal-select cfg-select" class:open={openDropdown === "settings-container"}>
                                <button
                                    type="button"
                                    class="terminal-select-btn"
                                    aria-haspopup="listbox"
                                    aria-expanded={openDropdown === "settings-container"}
                                    onclick={() => toggleDropdown("settings-container")}
                                    onkeydown={(e) =>
                                        handleDropdownKey(
                                            e,
                                            "settings-container",
                                            containerOptions,
                                            settings.container || "mp4",
                                            (value) => (settings.container = value),
                                        )}
                                >
                                    <span>{selectedLabel(containerOptions, settings.container || "mp4")}</span>
                                    <span class="terminal-caret">▾</span>
                                </button>
                                {#if openDropdown === "settings-container"}
                                    <div class="terminal-menu" role="listbox">
                                        {#each containerOptions as option}
                                            <button
                                                type="button"
                                                class:sel={(settings.container || "mp4") === option.value}
                                                role="option"
                                                aria-selected={(settings.container || "mp4") === option.value}
                                                onclick={() => {
                                                    settings.container = option.value;
                                                    closeDropdown();
                                                }}
                                            >
                                                <span class="terminal-check"
                                                    >{(settings.container || "mp4") === option.value
                                                        ? ">"
                                                        : ""}</span
                                                >
                                                {option.label}
                                            </button>
                                        {/each}
                                    </div>
                                {/if}
                            </div>
                        </div>
                        <div class="cfg">
                            <label>audio_codec</label>
                            <div class="terminal-select cfg-select" class:open={openDropdown === "settings-audio-codec"}>
                                <button
                                    type="button"
                                    class="terminal-select-btn"
                                    aria-haspopup="listbox"
                                    aria-expanded={openDropdown === "settings-audio-codec"}
                                    onclick={() => toggleDropdown("settings-audio-codec")}
                                    onkeydown={(e) =>
                                        handleDropdownKey(
                                            e,
                                            "settings-audio-codec",
                                            audioCodecOptions,
                                            settings.audio_codec || "aac",
                                            (value) => (settings.audio_codec = value),
                                        )}
                                >
                                    <span>{selectedLabel(audioCodecOptions, settings.audio_codec || "aac")}</span>
                                    <span class="terminal-caret">▾</span>
                                </button>
                                {#if openDropdown === "settings-audio-codec"}
                                    <div class="terminal-menu" role="listbox">
                                        {#each audioCodecOptions as option}
                                            <button
                                                type="button"
                                                class:sel={(settings.audio_codec || "aac") === option.value}
                                                role="option"
                                                aria-selected={(settings.audio_codec || "aac") === option.value}
                                                onclick={() => {
                                                    settings.audio_codec = option.value;
                                                    closeDropdown();
                                                }}
                                            >
                                                <span class="terminal-check"
                                                    >{(settings.audio_codec || "aac") === option.value
                                                        ? ">"
                                                        : ""}</span
                                                >
                                                {option.label}
                                            </button>
                                        {/each}
                                    </div>
                                {/if}
                            </div>
                        </div>
                        <div class="cfg">
                            <span class="cfg-label">focused_window</span>
                            <button
                                type="button"
                                class="terminal-toggle"
                                class:on={settings.audio_focused_window ?? true}
                                aria-pressed={settings.audio_focused_window ?? true}
                                onclick={() =>
                                    updateAudioToggle(
                                        "audio_focused_window",
                                        !(settings.audio_focused_window ?? true),
                                    )}
                            >
                                <span>{settings.audio_focused_window ?? true ? "on" : "off"}</span>
                            </button>
                        </div>
                        <div class="cfg">
                            <span class="cfg-label">discord</span>
                            <button
                                type="button"
                                class="terminal-toggle"
                                class:on={settings.audio_discord}
                                aria-pressed={!!settings.audio_discord}
                                onclick={() =>
                                    updateAudioToggle(
                                        "audio_discord",
                                        !settings.audio_discord,
                                    )}
                            >
                                <span>{settings.audio_discord ? "on" : "off"}</span>
                            </button>
                        </div>
                        <div class="cfg">
                            <span class="cfg-label">spotify</span>
                            <button
                                type="button"
                                class="terminal-toggle"
                                class:on={settings.audio_spotify}
                                aria-pressed={!!settings.audio_spotify}
                                onclick={() =>
                                    updateAudioToggle(
                                        "audio_spotify",
                                        !settings.audio_spotify,
                                    )}
                            >
                                <span>{settings.audio_spotify ? "on" : "off"}</span>
                            </button>
                        </div>
                        <div class="cfg">
                            <span class="cfg-label">microphone</span>
                            <button
                                type="button"
                                class="terminal-toggle"
                                class:on={settings.audio_microphone}
                                aria-pressed={!!settings.audio_microphone}
                                onclick={() =>
                                    updateAudioToggle(
                                        "audio_microphone",
                                        !settings.audio_microphone,
                                    )}
                            >
                                <span>{settings.audio_microphone ? "on" : "off"}</span>
                            </button>
                        </div>
                        <div class="cfg">
                            <span class="cfg-label">microphone_source</span>
                            <div class="terminal-select cfg-select" class:open={openDropdown === "settings-mic-source"}>
                                <button
                                    type="button"
                                    class="terminal-select-btn"
                                    aria-haspopup="listbox"
                                    aria-expanded={openDropdown === "settings-mic-source"}
                                    disabled={audioInputDevicesLoading}
                                    onclick={() => toggleDropdown("settings-mic-source")}
                                    onkeydown={(e) =>
                                        handleDropdownKey(
                                            e,
                                            "settings-mic-source",
                                            audioInputOptions,
                                            settings.audio_microphone_source || "default_input",
                                            (value) => updateAudioSetting("audio_microphone_source", value),
                                        )}
                                >
                                    <span>{audioInputDevicesLoading
                                        ? "scanning..."
                                        : selectedLabel(audioInputOptions, settings.audio_microphone_source || "default_input")}</span>
                                    <span class="terminal-caret">▾</span>
                                </button>
                                {#if openDropdown === "settings-mic-source"}
                                    <div class="terminal-menu" role="listbox">
                                        {#each audioInputOptions as option}
                                            <button
                                                type="button"
                                                class:sel={(settings.audio_microphone_source || "default_input") === option.value}
                                                role="option"
                                                aria-selected={(settings.audio_microphone_source || "default_input") === option.value}
                                                title={option.value}
                                                onclick={() => {
                                                    updateAudioSetting("audio_microphone_source", option.value);
                                                    closeDropdown();
                                                }}
                                            >
                                                <span class="terminal-check"
                                                    >{(settings.audio_microphone_source || "default_input") === option.value
                                                        ? ">"
                                                        : ""}</span
                                                >
                                                {option.label}
                                            </button>
                                        {/each}
                                    </div>
                                {/if}
                            </div>
                        </div>
                        <div class="cfg">
                            <label>audio_source</label><input
                                bind:value={settings.audio_source}
                            />
                        </div>
                        <div class="cfg">
                            <span class="cfg-label">clip_sound</span>
                            <button
                                type="button"
                                class="terminal-toggle"
                                class:on={settings.clipping_sound_enabled ?? true}
                                aria-pressed={settings.clipping_sound_enabled ?? true}
                                onclick={() =>
                                    updateAudioToggle(
                                        "clipping_sound_enabled",
                                        !(settings.clipping_sound_enabled ?? true),
                                    )}
                            >
                                <span>{settings.clipping_sound_enabled ?? true
                                    ? "on"
                                    : "off"}</span>
                            </button>
                        </div>
                        <div class="cfg">
                            <label for="clip-sound-path">clip_sound_path</label>
                            <div class="cfg-sound">
                                <input
                                    id="clip-sound-path"
                                    bind:value={settings.clipping_sound_path}
                                    placeholder="built-in chime"
                                />
                                <button
                                    type="button"
                                    class="act-sec"
                                    onclick={previewClippingSound}
                                    title="Preview clipping sound">▶</button
                                >
                            </div>
                        </div>
                    </section>
                    <section class="cfg-section">
                        <h3>── hotkeys</h3>
                        <div class="cfg">
                            <label>save_replay</label><input
                                bind:value={settings.hotkey_save_replay}
                            />
                        </div>
                        <div class="cfg">
                            <label>toggle_recording</label><input
                                bind:value={
                                    settings.hotkey_start_stop_recording
                                }
                            />
                        </div>
                        <div class="cfg">
                            <label>toggle_buffer</label><input
                                bind:value={settings.hotkey_start_stop_buffer}
                            />
                        </div>
                        <p class="cfg-hint">
                            → configured via <code
                                >~/.config/klyppd/save-replay.sh</code
                            >
                        </p>
                    </section>
                    <section class="cfg-section">
                        <h3>── cloudflare r2</h3>
                        <div class="cfg">
                            <label>endpoint</label><input
                                bind:value={settings.r2_endpoint}
                                placeholder="https://…r2.cloudflarestorage.com"
                            />
                        </div>
                        <div class="cfg">
                            <label>bucket</label><input
                                bind:value={settings.r2_bucket}
                            />
                        </div>
                        <div class="cfg">
                            <label>access_key</label><input
                                bind:value={settings.r2_access_key}
                            />
                        </div>
                        <div class="cfg">
                            <label>secret_key</label><input
                                type="password"
                                bind:value={settings.r2_secret_key}
                            />
                        </div>
                        <div class="cfg">
                            <label>custom_domain</label><input
                                bind:value={settings.r2_custom_domain}
                                placeholder="https://cdn.example.com"
                            />
                        </div>
                        <div class="cfg">
                            <label>expiry_days</label>
                            <div class="cfg-suffix">
                                <input
                                    type="number"
                                    bind:value={settings.expiry_days}
                                /><span>d</span>
                            </div>
                        </div>
                    </section>

                    <section class="cfg-section">
                        <button
                            class="theme-toggle"
                            onclick={() => (themeExpanded = !themeExpanded)}
                        >
                            <h3>── appearance</h3>
                            <span class="theme-chevron"
                                >{themeExpanded ? "▾" : "▸"}</span
                            >
                        </button>

                        {#if themeExpanded}
                            <div class="theme-editor">
                                <div class="theme-group">
                                    <span class="theme-group-label"
                                        >── transparency</span
                                    >
                                    <div class="cfg">
                                        <label>opacity</label>
                                        <div class="slider-wrap">
                                            <input
                                                type="range"
                                                min="0.1"
                                                max="1"
                                                step="0.05"
                                                value={currentTheme[
                                                    "--bg-opacity"
                                                ]}
                                                oninput={(e) => {
                                                    const v = (
                                                        e.target as HTMLInputElement
                                                    ).value;
                                                    setThemeVar(
                                                        "--bg-opacity",
                                                        v,
                                                    );
                                                    void applyWindowOpacity(v);
                                                }}
                                            />
                                            <span class="slider-val"
                                                >{parseFloat(
                                                    currentTheme[
                                                        "--bg-opacity"
                                                    ],
                                                ).toFixed(2)}</span
                                            >
                                        </div>
                                    </div>
                                    <div class="cfg">
                                        <label>blur</label>
                                        <div class="slider-wrap">
                                            <input
                                                type="range"
                                                min="0"
                                                max="32"
                                                step="1"
                                                value={parseInt(
                                                    currentTheme[
                                                        "--blur-radius"
                                                    ],
                                                )}
                                                oninput={(e) =>
                                                    setThemeVar(
                                                        "--blur-radius",
                                                        (
                                                            e.target as HTMLInputElement
                                                        ).value + "px",
                                                    )}
                                            />
                                            <span class="slider-val"
                                                >{currentTheme[
                                                    "--blur-radius"
                                                ]}</span
                                            >
                                        </div>
                                    </div>
                                </div>

                                <div class="theme-group">
                                    <span class="theme-group-label"
                                        >── colors</span
                                    >
                                    {#each colorVars as { key, label }}
                                        <div class="cfg color-cfg">
                                            <label>{label}</label>
                                            <div class="color-input">
                                                <input
                                                    type="color"
                                                    value={currentTheme[key]}
                                                    oninput={(e) =>
                                                        setThemeVar(
                                                            key,
                                                            (
                                                                e.target as HTMLInputElement
                                                            ).value,
                                                        )}
                                                />
                                                <input
                                                    type="text"
                                                    value={currentTheme[key]}
                                                    onchange={(e) =>
                                                        setThemeVar(
                                                            key,
                                                            (
                                                                e.target as HTMLInputElement
                                                            ).value,
                                                        )}
                                                />
                                            </div>
                                        </div>
                                    {/each}
                                </div>

                                <div class="theme-group">
                                    <span class="theme-group-label"
                                        >── typography</span
                                    >
                                    <div class="cfg">
                                        <label>font_size</label>
                                        <div class="slider-wrap">
                                            <input
                                                type="range"
                                                min="10"
                                                max="18"
                                                step="1"
                                                value={parseInt(
                                                    currentTheme["--font-size"],
                                                )}
                                                oninput={(e) =>
                                                    setThemeVar(
                                                        "--font-size",
                                                        (
                                                            e.target as HTMLInputElement
                                                        ).value + "px",
                                                    )}
                                            />
                                            <span class="slider-val"
                                                >{currentTheme[
                                                    "--font-size"
                                                ]}</span
                                            >
                                        </div>
                                    </div>
                                </div>

                                <div class="theme-actions">
                                    <button class="act-sec" onclick={resetTheme}
                                        >reset</button
                                    >
                                    <button
                                        class="act-sec"
                                        onclick={importThemeFromFile}
                                        >import .css</button
                                    >
                                    <button
                                        class="act-primary"
                                        onclick={exportTheme}>export</button
                                    >
                                </div>
                                <p class="cfg-hint">
                                    → theme file: <code
                                        >~/.config/klyppd/theme.css</code
                                    >
                                </p>
                                <p class="cfg-hint">
                                    → changes apply live and persist on restart
                                </p>
                            </div>
                        {/if}
                    </section>

                    <button class="save-btn" onclick={saveSettings}>save</button
                    >
                </div>
            {/if}
        </main>
    </div>
</div>

<style>
    /* ═══════════════════════════════════════════════════════════════════════
     KLYPPD — Terminal-Inspired Theme
     Designed to match CachyOS / Hyprland rice
     All colors are CSS-variable driven for full user customization.
     Supports window transparency + backdrop blur.
     ═══════════════════════════════════════════════════════════════════════ */

    :global(*) {
        box-sizing: border-box;
        margin: 0;
        padding: 0;
    }

    :global(html, body) {
        background: transparent;
        color: var(--text, #b8c4d0);
        font-family: var(
            --font-mono,
            "JetBrains Mono",
            "Fira Code",
            "Cascadia Code",
            "IBM Plex Mono",
            ui-monospace,
            monospace
        );
        font-size: var(--font-size, 12px);
        line-height: 1.55;
        height: 100%;
        overflow: hidden;
        -webkit-font-smoothing: antialiased;
        font-feature-settings:
            "liga" 1,
            "calt" 1;
    }

    :global(::-webkit-scrollbar) {
        display: none;
    }
    :global(*) {
        scrollbar-width: none;
    }

    .app {
        display: flex;
        flex-direction: column;
        height: 100vh;
        background: var(--bg-base-t, var(--bg-base, #0a0e14));
        backdrop-filter: blur(var(--blur-radius, 0px));
        -webkit-backdrop-filter: blur(var(--blur-radius, 0px));
    }

    /* ─── Boot Screen ────────────────────────────────────────────────── */

    .boot-screen {
        position: fixed;
        inset: 0;
        z-index: 2000;
        background: var(--bg-deepest, #080c12);
        color: var(--text-dim, #7a8899);
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        font-family: var(
            --font-mono,
            "JetBrains Mono",
            "Fira Code",
            "Cascadia Code",
            "IBM Plex Mono",
            ui-monospace,
            monospace
        );
    }
    .boot-terminal {
        width: min(760px, 92vw);
        min-height: min(380px, 70vh);
        border: 1px solid var(--border-strong, #232a35);
        background: var(--bg-base, #0a0e14);
        padding: 14px;
        display: flex;
        flex-direction: column;
    }
    .boot-terminal-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 14px;
        padding-bottom: 10px;
        border-bottom: 1px solid var(--border, #1a2030);
        color: var(--text-faint, #364050);
        font-size: 11px;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }
    .boot-terminal-lines {
        flex: 1;
        display: flex;
        flex-direction: column;
        justify-content: flex-end;
        gap: 7px;
        padding-top: 18px;
        color: var(--text-dim, #7a8899);
        font-size: clamp(11px, 1.7vw, 14px);
        line-height: 1.45;
        white-space: pre-wrap;
    }
    .boot-terminal-lines div:first-child {
        color: var(--accent, #56b6c2);
    }
    .boot-cursor {
        color: var(--accent, #56b6c2);
        animation: boot-cursor 1s steps(1, end) infinite;
    }
    .boot-progress-label {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 14px;
        margin-top: 18px;
        margin-bottom: 8px;
        font-size: 11px;
        color: var(--text-muted, #4a5568);
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }
    .boot-bar {
        height: 13px;
        border: 1px solid var(--border-strong, #232a35);
        background: var(--bg-deepest, #080c12);
        padding: 2px;
    }
    .boot-bar-fill {
        height: 100%;
        background: var(--accent, #56b6c2);
        transition: width 0.45s linear;
    }
    @keyframes boot-cursor {
        50% {
            opacity: 0;
        }
    }
    /* ─── Toast ───────────────────────────────────────────────────────── */

    .toast {
        position: fixed;
        bottom: 16px;
        left: 50%;
        transform: translateX(-50%);
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        color: var(--text, #b8c4d0);
        padding: 8px 14px;
        border-radius: 4px;
        font-size: 11px;
        z-index: 999;
        display: flex;
        align-items: center;
        gap: 8px;
        box-shadow: 0 4px 20px rgba(0, 0, 0, 0.6);
        animation: toast-in 0.2s ease-out;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }
    .toast.err {
        border-color: var(--danger, #e06c75);
        color: var(--danger, #e06c75);
    }
    .toast-prefix {
        color: var(--accent, #56b6c2);
        font-weight: 600;
    }
    .toast.err .toast-prefix {
        color: var(--danger, #e06c75);
    }
    @keyframes toast-in {
        from {
            opacity: 0;
            transform: translate(-50%, 8px);
        }
        to {
            opacity: 1;
            transform: translate(-50%, 0);
        }
    }

    /* ─── Upload Queue ───────────────────────────────────────────────── */

    .upload-dock {
        position: fixed;
        right: 14px;
        bottom: 14px;
        width: min(360px, calc(100vw - 28px));
        background: var(--bg-deepest-t, var(--bg-deepest, #080c12));
        border: 1px solid var(--border, #1a2030);
        border-radius: 4px;
        z-index: 950;
        overflow: hidden;
        box-shadow: 0 14px 42px rgba(0, 0, 0, 0.5);
        backdrop-filter: blur(var(--blur-radius, 0px));
        animation: upload-dock-in 0.18s ease-out;
    }
    .upload-dock.closed {
        width: min(260px, calc(100vw - 28px));
    }
    .upload-dock-head {
        width: 100%;
        min-height: 34px;
        padding: 8px 10px;
        display: flex;
        align-items: center;
        gap: 8px;
        background: transparent;
        border: none;
        color: var(--text-dim, #7a8899);
        font-family: inherit;
        font-size: 11px;
        cursor: pointer;
        text-align: left;
    }
    .upload-dock-head:hover {
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
    }
    .upload-dock-head > span:first-child {
        color: var(--accent, #56b6c2);
    }
    .upload-dock-meta {
        flex: 1;
        color: var(--text-faint, #364050);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .upload-dock-toggle {
        color: var(--text-muted, #4a5568);
    }
    .upload-queue {
        border-top: 1px solid var(--border, #1a2030);
        max-height: min(320px, calc(100vh - 96px));
        overflow-y: auto;
        padding: 6px;
    }
    .upload-item {
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        padding: 8px;
    }
    .upload-item + .upload-item {
        margin-top: 6px;
    }
    .upload-item.failed {
        border-color: var(--danger, #e06c75);
    }
    .upload-row {
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .upload-main {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 1px;
    }
    .upload-name {
        color: var(--text-dim, #7a8899);
        font-size: 11px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .upload-sub {
        color: var(--text-faint, #364050);
        font-size: 10px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .upload-actions {
        display: flex;
        gap: 4px;
        flex-shrink: 0;
    }
    .queue-btn,
    .clear-uploads {
        background: transparent;
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        color: var(--text-muted, #4a5568);
        cursor: pointer;
        font-family: inherit;
        font-size: 10px;
        padding: 2px 6px;
        white-space: nowrap;
    }
    .queue-btn:hover,
    .clear-uploads:hover {
        border-color: var(--border-strong, #232a35);
        color: var(--text-dim, #7a8899);
    }
    .upload-progress {
        height: 3px;
        margin-top: 7px;
        border-radius: 999px;
        background: var(--bg-deepest, #080c12);
        overflow: hidden;
    }
    .upload-progress-fill {
        height: 100%;
        background: var(--accent, #56b6c2);
        border-radius: inherit;
        transition: width 0.25s ease;
    }
    .upload-item.failed .upload-progress-fill {
        background: var(--danger, #e06c75);
    }
    .clear-uploads {
        width: 100%;
        margin-top: 6px;
        padding: 5px 8px;
    }
    @keyframes upload-dock-in {
        from {
            opacity: 0;
            transform: translateY(8px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }

    /* ─── Header ──────────────────────────────────────────────────────── */

    header {
        display: flex;
        align-items: center;
        height: 36px;
        padding: 0 12px;
        background: var(--bg-deepest-t, var(--bg-deepest, #080c12));
        border-bottom: 1px solid var(--border, #1a2030);
        -webkit-app-region: drag;
        gap: 0;
        font-size: 11px;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }

    .brand {
        display: flex;
        align-items: center;
        gap: 8px;
        -webkit-app-region: drag;
    }
    .brand img {
        height: 26px;
        width: auto;
        display: block;
        opacity: 0.9;
    }
    .brand-sep {
        color: var(--border-strong, #232a35);
        font-size: 14px;
        margin-right: 10px;
    }

    .header-info {
        display: flex;
        align-items: center;
        gap: 6px;
        flex: 1;
        -webkit-app-region: no-drag;
    }

    .rec-btn {
        display: flex;
        align-items: center;
        gap: 5px;
        padding: 2px 10px;
        background: transparent;
        border: 1px solid var(--border, #1a2030);
        color: var(--text-muted, #4a5568);
        border-radius: 3px;
        font-size: 11px;
        cursor: pointer;
        transition: all 0.15s;
        font-family: inherit;
        letter-spacing: 0.3px;
    }
    .rec-btn:hover {
        border-color: var(--border-strong, #232a35);
        color: var(--text-dim, #7a8899);
    }
    .rec-btn.on {
        border-color: var(--accent, #56b6c2);
        color: var(--accent, #56b6c2);
    }
    .rec-indicator {
        font-size: 8px;
    }
    .rec-btn.on .rec-indicator {
        animation: blink 1.4s ease infinite;
    }
    @keyframes blink {
        50% {
            opacity: 0.3;
        }
    }

    .info-tag {
        font-size: 10px;
        color: var(--text-faint, #364050);
        padding: 1px 6px;
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border-radius: 2px;
        letter-spacing: 0.4px;
    }

    .window-ctrls {
        -webkit-app-region: no-drag;
        display: flex;
        gap: 1px;
    }
    .window-ctrls button {
        width: 28px;
        height: 28px;
        background: none;
        border: none;
        color: var(--text-faint, #364050);
        border-radius: 3px;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 11px;
        font-family: inherit;
        transition: all 0.1s;
    }
    .window-ctrls .wc-min:hover {
        background: var(--bg-elev-1, #111620);
        color: var(--text-dim, #7a8899);
    }
    .window-ctrls .wc-close:hover {
        background: var(--danger, #e06c75);
        color: var(--bg-base, #0a0e14);
    }

    /* ─── Layout ──────────────────────────────────────────────────────── */

    .layout {
        display: flex;
        flex: 1;
        min-height: 0;
    }

    .sidenav {
        width: 148px;
        background: var(--bg-deepest-t, var(--bg-deepest, #080c12));
        border-right: 1px solid var(--border, #1a2030);
        padding: 8px 6px;
        display: flex;
        flex-direction: column;
        gap: 0;
        transition: width 0.2s ease;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }
    .sidenav.collapsed {
        width: 44px;
    }

    .collapse-toggle {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 22px;
        background: none;
        border: none;
        color: var(--text-faint, #364050);
        cursor: pointer;
        font-size: 12px;
        font-family: inherit;
        margin-bottom: 4px;
        border-radius: 3px;
        transition: all 0.1s;
    }
    .collapse-toggle:hover {
        color: var(--text-muted, #4a5568);
        background: var(--bg-elev-1, #111620);
    }

    .nav-items {
        display: flex;
        flex-direction: column;
        gap: 1px;
    }

    .sidenav button {
        display: flex;
        align-items: center;
        justify-content: flex-start;
        gap: 8px;
        padding: 6px 10px;
        background: none;
        border: none;
        color: var(--text-muted, #4a5568);
        border-radius: 3px;
        cursor: pointer;
        font-size: 12px;
        font-family: inherit;
        text-align: left;
        transition: all 0.1s;
        white-space: nowrap;
        overflow: hidden;
        width: 100%;
    }
    .sidenav.collapsed button {
        justify-content: center;
        padding: 6px;
    }
    .sidenav button:hover {
        background: var(--bg-elev-1, #111620);
        color: var(--text-dim, #7a8899);
    }
    .sidenav button.sel {
        color: var(--accent, #56b6c2);
        background: rgba(86, 182, 194, 0.06);
    }

    .nav-icon {
        width: 16px;
        height: 16px;
        flex-shrink: 0;
        display: block;
        stroke-width: 1.8;
        stroke-linecap: round;
        stroke-linejoin: round;
    }
    .nav-label {
        font-size: 12px;
    }
    .nav-spacer {
        flex: 1;
    }

    main {
        flex: 1;
        overflow-y: scroll;
        -ms-overflow-style: none;
        scrollbar-width: none;
        padding: 18px 22px;
        background: var(--bg-base, #101418);
    }
    main::-webkit-scrollbar { width: 0; height: 0; display: none; }

    /* ─── Page Head ───────────────────────────────────────────────────── */

    .page-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 2px;
    }
    .page-title {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .prompt {
        color: var(--accent, #56b6c2);
        font-weight: 600;
        font-size: 13px;
    }
    .page-head h1 {
        font-size: 14px;
        font-weight: 500;
        color: var(--text, #b8c4d0);
        letter-spacing: 0.3px;
    }
    .count {
        font-size: 11px;
        color: var(--text-faint, #364050);
        background: var(--bg-elev-1, #111620);
        padding: 1px 6px;
        border-radius: 2px;
    }
    .page-stat {
        font-size: 11px;
        color: var(--text-faint, #364050);
    }

    .page-controls {
        display: flex;
        align-items: center;
        gap: 2px;
    }
    .library-search,
    .terminal-select-btn {
        height: 26px;
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        color: var(--text-dim, #7a8899);
        border-radius: 3px;
        font-size: 11px;
        font-family: inherit;
        outline: none;
    }
    .library-search {
        appearance: none;
        -webkit-appearance: none;
    }
    .library-search {
        width: 150px;
        padding: 0 8px;
    }
    .library-search::placeholder {
        color: var(--text-faint, #364050);
    }
    .terminal-select {
        position: relative;
        min-width: 90px;
    }
    .terminal-select-btn {
        width: 100%;
        min-width: 90px;
        padding: 0 8px;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        text-align: left;
    }
    .terminal-caret {
        color: var(--text-faint, #364050);
        font-size: 10px;
    }
    .library-search:focus,
    .terminal-select.open .terminal-select-btn,
    .terminal-select-btn:focus {
        border-color: var(--accent, #56b6c2);
        color: var(--text, #b8c4d0);
    }
    .terminal-menu {
        position: absolute;
        z-index: 80;
        top: calc(100% + 4px);
        left: 0;
        min-width: 100%;
        background: var(--bg-deepest-t, var(--bg-deepest, #080c12));
        border: 1px solid var(--border-strong, #232a35);
        border-radius: 3px;
        padding: 4px;
        box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
        backdrop-filter: blur(var(--blur-radius, 0px));
    }
    .terminal-menu button {
        width: 100%;
        min-height: 24px;
        padding: 3px 8px;
        display: flex;
        align-items: center;
        gap: 6px;
        background: transparent;
        border: none;
        border-radius: 2px;
        color: var(--text-muted, #4a5568);
        cursor: pointer;
        font-family: inherit;
        font-size: 11px;
        text-align: left;
        white-space: nowrap;
    }
    .terminal-menu button:hover,
    .terminal-menu button.sel {
        background: rgba(86, 182, 194, 0.08);
        color: var(--accent, #56b6c2);
    }
    .terminal-check {
        width: 8px;
        color: var(--accent, #56b6c2);
        flex-shrink: 0;
    }
    .terminal-toggle {
        min-width: 58px;
        height: 26px;
        padding: 0 9px;
        background: var(--bg-base, #0a0e14);
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        color: var(--text-faint, #364050);
        cursor: pointer;
        font-family: inherit;
        font-size: 11px;
        text-align: center;
        text-transform: uppercase;
    }
    .terminal-toggle.on {
        border-color: var(--accent, #56b6c2);
        color: var(--accent, #56b6c2);
        background: rgba(86, 182, 194, 0.07);
    }
    .terminal-toggle:hover {
        border-color: var(--border-strong, #232a35);
        color: var(--text-dim, #7a8899);
    }
    .terminal-toggle.on:hover {
        border-color: var(--accent, #56b6c2);
        color: var(--accent, #56b6c2);
    }
    .cfg-select {
        width: 100%;
    }
    .view-toggle {
        width: 26px;
        height: 26px;
        background: none;
        border: 1px solid transparent;
        color: var(--text-faint, #364050);
        border-radius: 3px;
        cursor: pointer;
        font-size: 13px;
        font-family: inherit;
        display: flex;
        align-items: center;
        justify-content: center;
        transition: all 0.1s;
    }
    .view-toggle:hover {
        color: var(--text-muted, #4a5568);
    }
    .view-toggle.active {
        color: var(--accent, #56b6c2);
        border-color: var(--border, #1a2030);
    }

    .separator {
        height: 1px;
        background: var(--border, #1a2030);
        margin: 10px 0 16px;
    }

    /* ─── Empty State ─────────────────────────────────────────────────── */

    .empty {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        padding: 48px 20px;
        text-align: center;
        color: var(--text-muted, #4a5568);
    }
    .empty-art {
        color: var(--text-faint, #364050);
        font-size: 12px;
        line-height: 1.4;
        margin-bottom: 12px;
    }
    .empty p {
        font-size: 11px;
        color: var(--text-muted, #4a5568);
    }
    .empty kbd {
        background: var(--bg-elev-1, #111620);
        border: 1px solid var(--border, #1a2030);
        padding: 1px 5px;
        border-radius: 3px;
        font-family: inherit;
        font-size: 11px;
        color: var(--accent, #56b6c2);
    }

    /* ─── Grid View ───────────────────────────────────────────────────── */

    .grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
        gap: 10px;
    }

    .clip-card {
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        border-radius: 4px;
        overflow: hidden;
        transition: border-color 0.15s;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }
    .clip-card:hover {
        border-color: var(--border-strong, #232a35);
    }
    .clip-card.fav {
        border-color: var(--accent, #56b6c2);
        box-shadow: inset 0 0 0 1px rgba(86, 182, 194, 0.18);
    }

    .clip-thumb {
        position: relative;
        aspect-ratio: 16/9;
        background: var(--bg-deepest, #080c12);
        cursor: pointer;
        overflow: hidden;
    }
    .clip-thumb img {
        width: 100%;
        height: 100%;
        object-fit: cover;
        transition: transform 0.25s ease;
    }
    .clip-thumb:hover img {
        transform: scale(1.03);
    }
    .thumb-placeholder {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--text-faint, #364050);
        font-size: 20px;
    }
    .clip-dur {
        position: absolute;
        bottom: 6px;
        right: 6px;
        background: rgba(0, 0, 0, 0.8);
        color: var(--text-dim, #7a8899);
        padding: 1px 5px;
        border-radius: 2px;
        font-size: 10px;
        letter-spacing: 0.3px;
    }
    .clip-hover {
        position: absolute;
        inset: 0;
        background: rgba(0, 0, 0, 0.55);
        display: flex;
        align-items: center;
        justify-content: center;
        opacity: 0;
        transition: opacity 0.12s;
        color: var(--accent, #56b6c2);
        font-size: 12px;
        letter-spacing: 0.5px;
    }
    .clip-thumb:hover .clip-hover {
        opacity: 1;
    }

    .clip-info {
        padding: 8px 10px 4px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .clip-name {
        font-size: 12px;
        color: var(--text-dim, #7a8899);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        cursor: text;
    }
    .clip-time {
        font-size: 10px;
        color: var(--text-faint, #364050);
    }

    .clip-actions {
        display: flex;
        gap: 3px;
        padding: 4px 8px 8px;
        flex-wrap: wrap;
    }

    /* ─── List View ───────────────────────────────────────────────────── */

    .list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .list-row {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 6px 10px;
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        cursor: pointer;
        transition: border-color 0.1s;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }
    .list-row:hover {
        border-color: var(--border-strong, #232a35);
    }
    .list-row.fav {
        border-color: var(--accent, #56b6c2);
        box-shadow: inset 0 0 0 1px rgba(86, 182, 194, 0.14);
    }

    .list-thumb {
        width: 56px;
        height: 32px;
        border-radius: 2px;
        overflow: hidden;
        background: var(--bg-deepest, #080c12);
        flex-shrink: 0;
    }
    .list-thumb img {
        width: 100%;
        height: 100%;
        object-fit: cover;
    }
    .list-thumb-ph {
        width: 100%;
        height: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--text-faint, #364050);
        font-size: 11px;
    }

    .list-meta {
        flex: 1;
        min-width: 0;
    }
    .list-name {
        font-size: 12px;
        color: var(--text-dim, #7a8899);
        display: block;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .list-sub {
        font-size: 10px;
        color: var(--text-faint, #364050);
    }

    .list-actions {
        display: flex;
        gap: 3px;
        flex-shrink: 0;
    }

    /* ─── Buttons ─────────────────────────────────────────────────────── */

    .act-primary {
        padding: 3px 10px;
        background: rgba(86, 182, 194, 0.08);
        border: 1px solid rgba(86, 182, 194, 0.2);
        color: var(--accent, #56b6c2);
        border-radius: 3px;
        font-size: 11px;
        font-family: inherit;
        cursor: pointer;
        transition: all 0.12s;
        white-space: nowrap;
    }
    .act-primary:hover {
        background: rgba(86, 182, 194, 0.14);
        border-color: rgba(86, 182, 194, 0.35);
    }
    .act-primary:disabled {
        opacity: 0.3;
        cursor: not-allowed;
    }

    .act-sec {
        padding: 3px 8px;
        background: transparent;
        border: 1px solid var(--border, #1a2030);
        color: var(--text-muted, #4a5568);
        border-radius: 3px;
        font-size: 11px;
        font-family: inherit;
        cursor: pointer;
        transition: all 0.12s;
        white-space: nowrap;
    }
    .act-sec:hover {
        border-color: var(--border-strong, #232a35);
        color: var(--text-dim, #7a8899);
    }
    .act-sec.pin.on {
        color: var(--accent, #56b6c2);
        border-color: var(--accent, #56b6c2);
        background: rgba(86, 182, 194, 0.06);
    }
    .act-sec:disabled {
        opacity: 0.3;
        cursor: not-allowed;
    }
    .act-sec.del:hover,
    .del:hover {
        color: var(--danger, #e06c75);
        border-color: var(--danger, #e06c75);
    }

    /* ─── Rows (Uploads) ──────────────────────────────────────────────── */

    .rows {
        display: flex;
        flex-direction: column;
        gap: 3px;
    }

    .row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 8px 12px;
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        transition: border-color 0.1s;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }
    .row:hover {
        border-color: var(--border-strong, #232a35);
    }

    .row-info {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
        flex: 1;
    }
    .row-name {
        font-size: 12px;
        color: var(--text-dim, #7a8899);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        cursor: text;
    }
    .row-tag {
        font-size: 10px;
        color: var(--text-faint, #364050);
        flex-shrink: 0;
    }
    .row-btns {
        display: flex;
        gap: 4px;
        flex-shrink: 0;
    }

    /* ─── Editor ──────────────────────────────────────────────────────── */

    .editor {
        display: flex;
        flex-direction: column;
        gap: 12px;
        height: calc(100vh - 80px);
        margin: -18px -22px 0;
        padding: 12px 18px;
        background: var(--bg-deepest-t, var(--bg-deepest, #080c12));
        backdrop-filter: blur(var(--blur-radius, 0px));
    }

    .editor-bar {
        display: flex;
        align-items: center;
        gap: 10px;
    }
    .ed-spacer {
        flex: 1;
    }
    .ed-back {
        padding: 3px 10px;
        background: transparent;
        border: 1px solid var(--border, #1a2030);
        color: var(--text-muted, #4a5568);
        border-radius: 3px;
        font-size: 11px;
        font-family: inherit;
        cursor: pointer;
        transition: all 0.1s;
    }
    .ed-back:hover {
        border-color: var(--border-strong, #232a35);
        color: var(--text-dim, #7a8899);
    }
    .ed-filename {
        font-size: 12px;
        color: var(--text-muted, #4a5568);
        max-width: 360px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .audio-track-strip {
        display: flex;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
    }
    .audio-track-toggle {
        height: 26px;
        padding: 0 9px;
        display: inline-flex;
        align-items: center;
        gap: 7px;
        background: var(--bg-elev-1, #111620);
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        color: var(--text-dim, #7a8899);
        font-family: inherit;
        font-size: 11px;
        cursor: pointer;
        text-transform: lowercase;
    }
    .audio-track-toggle span {
        color: var(--accent, #56b6c2);
        font-size: 10px;
        text-transform: uppercase;
    }
    .audio-track-toggle.muted {
        color: var(--text-faint, #364050);
    }
    .audio-track-toggle.muted span {
        color: var(--danger, #e06c75);
    }
    .audio-track-toggle.mixed {
        border-style: dashed;
    }

    .video-wrap {
        flex: 1;
        min-height: 0;
        background: #000;
        border-radius: 3px;
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
        border: 1px solid var(--border, #1a2030);
    }
    .video-wrap video {
        width: 100%;
        height: 100%;
        object-fit: contain;
    }

    .vid-loading {
        color: var(--text-muted, #4a5568);
        font-size: 12px;
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .spinner {
        display: inline-block;
        width: 12px;
        height: 12px;
        border: 1.5px solid var(--border-strong, #232a35);
        border-top-color: var(--accent, #56b6c2);
        border-radius: 50%;
        animation: spin 0.7s linear infinite;
    }
    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }

    /* ─── Timeline ────────────────────────────────────────────────────── */

    .tl-section {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .tl-readout {
        display: flex;
        justify-content: space-between;
        font-size: 11px;
        color: var(--text-muted, #4a5568);
        letter-spacing: 0.3px;
    }
    .tl-trim {
        color: var(--accent, #56b6c2);
    }

    .tl {
        position: relative;
        height: 40px;
        background: var(--bg-elev-1, #111620);
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        cursor: pointer;
        user-select: none;
    }
    .tl-track {
        position: absolute;
        inset: 0;
    }
    .tl-region {
        position: absolute;
        top: 0;
        bottom: 0;
        background: rgba(86, 182, 194, 0.06);
        border-left: 2px solid var(--accent, #56b6c2);
        border-right: 2px solid var(--accent, #56b6c2);
    }
    .tl-handle {
        position: absolute;
        top: -1px;
        width: 12px;
        height: 42px;
        margin-left: -6px;
        background: var(--accent, #56b6c2);
        border-radius: 2px;
        cursor: ew-resize;
        z-index: 2;
        transition: transform 0.1s;
        opacity: 0.9;
    }
    .tl-handle:hover {
        opacity: 1;
        transform: scaleY(1.04);
    }
    .tl-handle::before {
        content: "";
        position: absolute;
        top: 50%;
        left: 50%;
        transform: translate(-50%, -50%);
        width: 2px;
        height: 14px;
        background: rgba(0, 0, 0, 0.5);
        border-radius: 1px;
    }
    .tl-cursor {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 1.5px;
        background: var(--text-dim, #7a8899);
        pointer-events: none;
        z-index: 1;
    }

    .tl-btns {
        display: flex;
        gap: 4px;
        justify-content: center;
    }

    /* ─── Settings ────────────────────────────────────────────────────── */

    .settings {
        max-width: 560px;
    }

    .cfg-section {
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        border-radius: 4px;
        padding: 4px 14px;
        margin-bottom: 8px;
        backdrop-filter: blur(var(--blur-radius, 0px));
    }

    .cfg-section h3 {
        font-size: 11px;
        font-weight: 500;
        color: var(--text-faint, #364050);
        letter-spacing: 0.5px;
        padding: 8px 0 6px;
        border-bottom: 1px solid var(--border, #1a2030);
    }

    .cfg {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 7px 0;
        border-bottom: 1px solid rgba(26, 32, 48, 0.5);
    }
    .cfg:last-child {
        border-bottom: none;
    }

    .cfg label,
    .cfg-label {
        font-size: 12px;
        color: var(--text-muted, #4a5568);
    }

    .cfg input {
        width: 220px;
        padding: 4px 8px;
        background: var(--bg-base, #0a0e14);
        border: 1px solid var(--border, #1a2030);
        color: var(--text, #b8c4d0);
        border-radius: 3px;
        font-size: 12px;
        font-family: inherit;
        outline: none;
        transition: border-color 0.1s;
        -webkit-appearance: none;
        appearance: none;
    }
    .cfg input:focus {
        border-color: var(--accent, #56b6c2);
    }
    .cfg input[type="number"] {
        appearance: textfield;
        -moz-appearance: textfield;
    }
    .cfg input[type="number"]::-webkit-outer-spin-button,
    .cfg input[type="number"]::-webkit-inner-spin-button {
        -webkit-appearance: none;
        margin: 0;
    }

    .cfg-suffix {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .cfg-suffix input {
        width: 80px;
    }
    .cfg-suffix span {
        font-size: 10px;
        color: var(--text-faint, #364050);
    }
    .cfg-sound {
        width: 220px;
        display: grid;
        grid-template-columns: minmax(0, 1fr) 28px;
        gap: 5px;
    }
    .cfg-sound input {
        width: 100%;
        min-width: 0;
    }
    .cfg-sound .act-sec {
        width: 28px;
        height: 26px;
        padding: 0;
    }

    .cfg-hint {
        font-size: 11px;
        color: var(--text-faint, #364050);
        padding: 6px 0;
        line-height: 1.5;
    }
    .cfg-hint code {
        background: var(--bg-base, #0a0e14);
        padding: 1px 4px;
        border-radius: 2px;
        font-size: 10px;
        color: var(--accent, #56b6c2);
    }

    .save-btn {
        padding: 6px 16px;
        background: rgba(86, 182, 194, 0.08);
        border: 1px solid rgba(86, 182, 194, 0.2);
        color: var(--accent, #56b6c2);
        border-radius: 3px;
        font-size: 12px;
        font-family: inherit;
        cursor: pointer;
        margin-top: 4px;
        transition: all 0.12s;
    }
    .save-btn:hover {
        background: rgba(86, 182, 194, 0.14);
        border-color: rgba(86, 182, 194, 0.35);
    }

    /* ─── Theme Editor ───────────────────────────────────────────────── */

    .theme-toggle {
        display: flex;
        align-items: center;
        justify-content: space-between;
        width: 100%;
        background: none;
        border: none;
        cursor: pointer;
        padding: 0;
        font-family: inherit;
    }
    .theme-toggle h3 {
        border-bottom: none !important;
        padding-bottom: 0 !important;
    }
    .theme-chevron {
        color: var(--text-faint, #364050);
        font-size: 11px;
        padding: 8px 0 6px;
    }

    .theme-editor {
        padding: 4px 0 8px;
    }

    .theme-group {
        margin-bottom: 8px;
    }
    .theme-group-label {
        display: block;
        font-size: 10px;
        color: var(--text-faint, #364050);
        letter-spacing: 0.5px;
        padding: 6px 0 4px;
    }

    .color-cfg .color-input {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .color-cfg input[type="color"] {
        width: 28px;
        height: 24px;
        padding: 0;
        border: 1px solid var(--border, #1a2030);
        border-radius: 3px;
        cursor: pointer;
        background: none;
    }
    .color-cfg input[type="color"]::-webkit-color-swatch-wrapper {
        padding: 2px;
    }
    .color-cfg input[type="color"]::-webkit-color-swatch {
        border: none;
        border-radius: 2px;
    }
    .color-cfg input[type="text"] {
        width: 90px;
        font-size: 11px;
    }

    .slider-wrap {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .slider-wrap input[type="range"] {
        width: 140px;
        height: 4px;
        -webkit-appearance: none;
        appearance: none;
        background: var(--bg-elev-2, #161b23);
        border: none;
        border-radius: 2px;
        outline: none;
        padding: 0;
    }
    .slider-wrap input[type="range"]::-webkit-slider-thumb {
        -webkit-appearance: none;
        width: 12px;
        height: 12px;
        border-radius: 50%;
        background: var(--accent, #56b6c2);
        cursor: pointer;
        border: none;
    }
    .slider-val {
        font-size: 10px;
        color: var(--text-dim, #7a8899);
        min-width: 36px;
        text-align: right;
        font-family: inherit;
    }

    .theme-actions {
        display: flex;
        gap: 4px;
        padding: 8px 0 4px;
    }
</style>
