<script lang="ts">
    import { onMount } from "svelte";
    import { theme, defaultTheme, type ThemeVars } from "./lib/stores/theme";

    function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
        // Map named args from Tauri-style { key: val } to positional IPC args
        if (args) {
            const keys = Object.keys(args);
            if (keys.length === 1) {
                return window.klyppd.invoke(cmd, args[keys[0]]) as Promise<T>;
            }
            return window.klyppd.invoke(cmd, ...Object.values(args)) as Promise<T>;
        }
        return window.klyppd.invoke(cmd) as Promise<T>;
    }

    function listen(event: string, handler: (e: { payload: any }) => void): void {
        window.klyppd.on(event, (payload: any) => handler({ payload }));
    }

    async function writeText(text: string): Promise<void> {
        await window.klyppd.clipboard.writeText(text);
    }

    type Tab = "library" | "uploads" | "permanent" | "settings" | "editor";
    type ToastKind = "ok" | "err";
    type ViewMode = "grid" | "list";

    type Clip = {
        id: string;
        filename: string;
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
    let searchQuery = $state("");
    let sortBy = $state<"date" | "name" | "duration">("date");
    let sortAsc = $state(false);
    let recordingStartTime = $state<number | null>(null);
    let recordingElapsed = $state("");
    let depsOk = $state(true);
    let depsMissing = $state<string[]>([]);

    let toast = $state<{ msg: string; kind: ToastKind } | null>(null);
    let toastTimer: any;

    let r2TempBytes = $state(0);
    let r2PermBytes = $state(0);

    let videoEl: HTMLVideoElement;
    let previewSrc = $state("");
    let previewLoading = $state(false);
    let currentTime = $state(0);
    let duration = $state(0);
    let trimStart = $state(0);
    let trimEnd = $state(10);

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

    // ─── Lifecycle ─────────────────────────────────────────────────────────

    onMount(async () => {
        await injectUserTheme();
        theme.apply();
        console.log("klyppd frontend: window.klyppd =", window.klyppd);
        settings = await invoke("get_settings");
        console.log("klyppd frontend: settings =", JSON.stringify(settings));

        try {
            const missing = await invoke<string[]>("check_dependencies");
            if (missing.length > 0) {
                depsOk = false;
                depsMissing = missing;
            }
        } catch {}

        await refresh();
        setInterval(refresh, 3000);
        setInterval(updateRecordingTimer, 500);
        await loadClips();

        listen("clip-saved", (e: any) => {
            notify("Clip saved: " + e.payload.filename);
            setTimeout(loadClips, 500);
        });
        listen("toast", (e: any) => notify(e.payload.msg, e.payload.kind));

        theme.subscribe((v) => {
            currentTheme = v;
        });
    });

    async function injectUserTheme() {
        const css = await invoke<string>("get_theme_css");
        if (!css) return;
        theme.importCSS(css);
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
        const prev = recState.recording_active;
        recState = await invoke("get_recording_state");
        if (recState.recording_active && !prev) {
            recordingStartTime = Date.now();
        } else if (!recState.recording_active && prev) {
            recordingStartTime = null;
            recordingElapsed = "";
        }
        if (tab !== "settings") settings = await invoke("get_settings");
        storageUsage = await invoke("get_storage_usage");
    }

    function updateRecordingTimer() {
        if (recordingStartTime) {
            const s = Math.floor((Date.now() - recordingStartTime) / 1000);
            const m = Math.floor(s / 60);
            const sec = (s % 60).toString().padStart(2, "0");
            recordingElapsed = `${m}:${sec}`;
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
        if (t === "uploads") {
            uploadedClips = await invoke("get_uploaded_clips", {
                permanent: false,
            });
            for (const c of uploadedClips)
                if (c.thumbnail_path) loadThumb(c.thumbnail_path);
            r2TempBytes = await invoke<number>("r2_storage", {
                permanent: false,
            }).catch(() => 0);
        }
        if (t === "permanent") {
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

    async function upload(clip: Clip, permanent: boolean) {
        try {
            const url = await invoke<string>("upload_clip", {
                id: clip.id,
                permanent,
            });
            await writeText(url);
            notify(permanent ? "Permanent link copied" : "Link copied");
        } catch {
            notify("Upload failed", "err");
        }
    }

    const quickShare = (c: Clip) => upload(c, false);
    const permUpload = (c: Clip) => upload(c, true);

    async function copyLink(clip: Clip) {
        if (!clip.r2_url) return;
        await writeText(clip.r2_url);
        notify("Link copied");
    }

    async function deleteClip(clip: Clip) {
        if (!confirm(`Delete "${clip.filename}"? This cannot be undone.`)) return;
        try {
            await invoke("delete_clip", { id: clip.id });
            notify("Deleted");
            await loadClips();
        } catch (e: any) {
            notify("Delete failed", "err");
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
        if (!confirm(`Remove "${clip.filename}" from R2? The remote file will be deleted.`)) return;
        try {
            await invoke("delete_from_r2", { id: clip.id });
            notify("Removed from R2");
            await switchTab(tab);
        } catch (e: any) {
            notify("R2 delete failed", "err");
        }
    }

    // ─── Editor ────────────────────────────────────────────────────────────

    async function openEditor(clip: Clip) {
        editClip = clip;
        trimStart = 0;
        trimEnd = clip.duration || 10;
        tab = "editor";
        previewSrc = "";
        previewLoading = true;

        try {
            const path = await invoke<string>("transcode_for_preview", {
                input: clip.path,
            });
            previewSrc = await invoke<string>("serve_video", { path });
        } catch (e) {
            console.error(e);
            notify("Preview failed", "err");
        } finally {
            previewLoading = false;
        }
    }

    function closeEditor() {
        previewSrc = "";
        editClip = null;
        tab = "library";
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

    // ─── Theme ─────────────────────────────────────────────────────────────

    function setThemeVar(key: keyof ThemeVars, value: string) {
        theme.updateVar(key, value);
    }

    function resetTheme() {
        theme.reset();
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

    function filteredClips(list: Clip[]) {
        let result = list;
        if (searchQuery.trim()) {
            const q = searchQuery.toLowerCase();
            result = result.filter(
                (c) =>
                    c.filename.toLowerCase().includes(q) ||
                    (c.tags && c.tags.toLowerCase().includes(q)) ||
                    (c.folder && c.folder.toLowerCase().includes(q)),
            );
        }
        result = [...result].sort((a, b) => {
            let cmp = 0;
            if (sortBy === "name") cmp = a.filename.localeCompare(b.filename);
            else if (sortBy === "duration") cmp = a.duration - b.duration;
            else cmp = a.created_at.localeCompare(b.created_at);
            return sortAsc ? cmp : -cmp;
        });
        return result;
    }
</script>

{#if toast}
    <div class="toast" class:err={toast.kind === "err"}>
        <span class="toast-prefix">{toast.kind === "err" ? "✗" : "→"}</span
        >{toast.msg}
    </div>
{/if}

<div class="app">
    <header data-tauri-drag-region>
        <div class="brand">
            <img src="./logo.png" alt="klyppd" />
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
            {#if recState.recording_active && recordingElapsed}
                <span class="info-tag rec-timer">{recordingElapsed}</span>
            {/if}
        </div>
        <div class="window-ctrls">
            <button
                onclick={() => window.klyppd.window.hide()}
                aria-label="hide"
                class="wc-min"
                title="Hide">─</button
            >
            <button
                onclick={() => window.klyppd.window.close()}
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
            {#if !depsOk}
                <div class="deps-warn">
                    <span class="deps-icon">⚠</span>
                    <span>Missing: {depsMissing.join(", ")}. Recording and editing features will not work.</span>
                </div>
            {/if}

            {#if tab === "library"}
                <div class="page-head">
                    <div class="page-title">
                        <span class="prompt">~</span>
                        <h1>library</h1>
                        <span class="count">{filteredClips(clips).length}</span>
                    </div>
                    <div class="page-controls">
                        <input
                            class="search-input"
                            type="text"
                            placeholder="search…"
                            bind:value={searchQuery}
                        />
                        <select class="sort-select" bind:value={sortBy} onchange={() => forceRepaint()}>
                            <option value="date">date</option>
                            <option value="name">name</option>
                            <option value="duration">duration</option>
                        </select>
                        <button class="sort-dir" onclick={() => { sortAsc = !sortAsc; forceRepaint(); }} title={sortAsc ? "Ascending" : "Descending"}>
                            {sortAsc ? "↑" : "↓"}
                        </button>
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
                {:else if viewMode === "grid"}
                    <div class="grid">
                        {#each filteredClips(clips) as clip}
                            <article class="clip-card">
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
                                            onclick={() => quickShare(clip)}
                                            >share</button
                                        >
                                        <button
                                            class="act-sec"
                                            onclick={() => permUpload(clip)}
                                            title="permanent upload">⬆</button
                                        >
                                    {/if}
                                    <button
                                        class="act-sec"
                                        onclick={() => deleteClip(clip)}
                                        title="delete">✕</button
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
                        {#each filteredClips(clips) as clip}
                            <div
                                class="list-row"
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
                                    {#if clip.r2_url}
                                        <button
                                            class="act-primary"
                                            onclick={() => copyLink(clip)}
                                            >copy</button
                                        >
                                    {:else}
                                        <button
                                            class="act-primary"
                                            onclick={() => quickShare(clip)}
                                            >share</button
                                        >
                                        <button
                                            class="act-sec"
                                            onclick={() => permUpload(clip)}
                                            >⬆</button
                                        >
                                    {/if}
                                    <button
                                        class="act-sec del"
                                        onclick={() => deleteClip(clip)}
                                        >✕</button
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
                            <article class="clip-card">
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
                                        class="act-primary"
                                        onclick={() => copyLink(clip)}
                                        >copy link</button
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
                            <article class="clip-card">
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
                                        class="act-primary"
                                        onclick={() => copyLink(clip)}
                                        >copy link</button
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
                            <label>codec</label><select
                                bind:value={settings.codec}
                                ><option>h264</option><option>hevc</option
                                ><option>av1</option></select
                            >
                        </div>
                        <div class="cfg">
                            <label>container</label><select
                                bind:value={settings.container}
                                ><option>mp4</option><option>mkv</option><option
                                    >webm</option
                                ></select
                            >
                        </div>
                        <div class="cfg">
                            <label>audio_codec</label><select
                                bind:value={settings.audio_codec}
                                ><option>aac</option><option>opus</option><option>flac</option></select
                            >
                        </div>
                        <div class="cfg">
                            <label>audio_source</label><input
                                bind:value={settings.audio_source}
                            />
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
                            → uses evdev for global hotkeys; add your user to the <code>input</code> group
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
                                                    invoke(
                                                        "set_window_opacity",
                                                        {
                                                            opacity:
                                                                parseFloat(v),
                                                        },
                                                    );
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
                                    → changes apply live — export to save
                                    permanently
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
        -webkit-app-region: drag;
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
        -webkit-app-region: no-drag;
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
    .sidenav.collapsed button {
        justify-content: center;
        padding: 6px;
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
        gap: 4px;
    }
    .view-toggle {
        width: 22px;
        height: 22px;
        background: none;
        border: 1px solid transparent;
        color: var(--text-faint, #364050);
        border-radius: 2px;
        cursor: pointer;
        font-size: 12px;
        font-family: var(--font-mono);
        display: flex;
        align-items: center;
        justify-content: center;
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
        grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
        gap: 10px;
        align-items: start;
    }

    .clip-card {
        background: var(--bg-elev-1-t, var(--bg-elev-1, #111620));
        border: 1px solid var(--border, #1a2030);
        border-radius: 4px;
        overflow: hidden;
        transition: border-color 0.15s;
        backdrop-filter: blur(var(--blur-radius, 0px));
        display: flex;
        flex-direction: column;
    }
    .clip-card:hover {
        border-color: var(--border-strong, #232a35);
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

    .cfg label {
        font-size: 12px;
        color: var(--text-muted, #4a5568);
    }

    .cfg input,
    .cfg select {
        width: 220px;
        padding: 4px 8px;
        background: var(--bg-base, #0a0e14);
        border: 1px solid var(--border, #1a2030);
        color: #e0e2e8;
        border-radius: 3px;
        font-size: 12px;
        font-family: inherit;
        outline: none;
        transition: border-color 0.1s;
        -webkit-appearance: none;
        appearance: none;
    }
    .cfg input:focus,
    .cfg select:focus {
        border-color: var(--accent, #56b6c2);
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

    .search-input {
        background: transparent;
        border: none;
        border-bottom: 1px solid var(--border);
        border-radius: 0;
        color: var(--text);
        font-family: var(--font-mono);
        font-size: 11px;
        padding: 4px 8px;
        width: 140px;
        outline: none;
    }
    .search-input::placeholder { color: var(--text-faint); }
    .search-input:focus { border-bottom-color: var(--accent); }
    .sort-select {
        background: transparent;
        border: none;
        border-bottom: 1px solid var(--border);
        border-radius: 0;
        color: var(--text-muted);
        font-family: var(--font-mono);
        font-size: 11px;
        padding: 4px 6px;
        outline: none;
        cursor: pointer;
        -webkit-appearance: none;
        appearance: none;
    }
    .sort-select:focus { border-bottom-color: var(--accent); }
    .sort-dir {
        background: transparent;
        border: 1px solid var(--border);
        border-radius: 2px;
        color: var(--text-muted);
        font-family: var(--font-mono);
        font-size: 12px;
        width: 22px;
        height: 22px;
        cursor: pointer;
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .sort-dir:hover { border-color: var(--accent); color: var(--accent); }

    .deps-warn {
        background: rgba(224, 108, 117, 0.12);
        border: 1px solid var(--danger);
        border-radius: 6px;
        padding: 8px 14px;
        margin: 8px 0;
        font-size: 12px;
        color: var(--danger);
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .deps-icon { font-size: 16px; }

    .rec-timer {
        color: #e06c75;
        font-weight: 600;
        animation: pulse-rec 1.2s ease-in-out infinite;
    }
    @keyframes pulse-rec {
        0%, 100% { opacity: 1; }
        50% { opacity: 0.5; }
    }
</style>
