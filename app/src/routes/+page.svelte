<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';

	// ---------------------------------------------------------------------------
	// Types
	// ---------------------------------------------------------------------------

	interface NotificationItem {
		title: string;
		body: string;
		app_name: string;
		timestamp_ms: number;
	}

	interface HistoryItem {
		id: string;
		app_name: string;
		title: string;
		body: string;
		producer_node: string;
		timestamp: string;
	}

	interface ProducerEntry {
		url: string;
		label: string | null;
		enabled: boolean;
	}

	interface ProducerStatus {
		url: string;
		state: 'connecting' | 'connected' | 'reconnecting' | 'unreachable';
		last_event_unix_ms: number | null;
		last_error: string | null;
	}

	interface Filter {
		field: 'title' | 'body' | 'appname' | 'any';
		contains: string;
		action: 'allow' | 'block';
	}

	interface Rules {
		default_mode: 'allow' | 'block';
		apps: Record<string, boolean>;
		filters: Filter[];
	}

	interface RetentionConfig {
		default_days: number;
		max_count: number | null;
		per_producer: Record<string, number>;
	}

	interface Focus {
		id: string;
		name: string;
		icon: string | null;
		rules: Rules;
		sort_order: number;
	}

	type WeekdayKey = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun';

	interface TimeRange {
		start_hhmm: number;
		end_hhmm: number;
	}

	interface FocusSchedule {
		id: string;
		focus_id: string;
		days: WeekdayKey[];
		time_range: TimeRange;
		enabled: boolean;
	}

	// ---------------------------------------------------------------------------
	// State (Svelte 5 runes)
	// ---------------------------------------------------------------------------

	type Panel = 'notifications' | 'history' | 'settings';
	type SettingsTab = 'producers' | 'filters' | 'retention' | 'focuses' | 'import-export' | 'theme';

	let activePanel = $state<Panel>('notifications');
	let settingsTab = $state<SettingsTab>('producers');

	// Notifications panel
	let notifications = $state<NotificationItem[]>([]);

	// History panel
	const HISTORY_PAGE_SIZE = 50;
	let historyItems = $state<HistoryItem[]>([]);
	let historyOffset = $state(0);
	let historyHasMore = $state(false);
	let historyFilter = $state('');
	let historyLoading = $state(false);
	let historyError = $state('');
	let pruneResult = $state<string | null>(null);
	let pruneLoading = $state(false);

	// Settings panel — producers list
	let producers = $state<ProducerEntry[]>([]);
	let statuses = $state<ProducerStatus[]>([]);

	// Add-producer form
	let newUrl = $state('');
	let newLabel = $state('');
	let addError = $state('');
	let addBusy = $state(false);

	// Filters panel
	let rules = $state<Rules>({ default_mode: 'allow', apps: {}, filters: [] });
	let seenApps = $state<string[]>([]);
	let filtersError = $state('');
	let filtersBusy = $state(false);

	// Add-filter form
	let newFilterField = $state<Filter['field']>('any');
	let newFilterContains = $state('');
	let newFilterAction = $state<Filter['action']>('block');

	// Retention settings
	let retention = $state<RetentionConfig>({ default_days: 30, max_count: null, per_producer: {} });
	let retentionError = $state('');
	let retentionBusy = $state(false);
	let retentionDefaultDaysInput = $state(30);
	let retentionMaxCountEnabled = $state(false);
	let retentionMaxCountInput = $state(1000);

	// Focuses
	let focuses = $state<Focus[]>([]);
	let activeFocusId = $state<string | null>(null);
	let focusesError = $state('');
	let focusesBusy = $state(false);

	// New focus form
	let newFocusName = $state('');
	let newFocusIcon = $state('');

	// Inline edit state for a focus in the settings list
	let editingFocusId = $state<string | null>(null);
	let editFocusName = $state('');
	let editFocusIcon = $state('');

	// Schedules
	let schedules = $state<FocusSchedule[]>([]);
	let schedulesError = $state('');
	let schedulesBusy = $state(false);

	// Add-schedule form
	const ALL_DAYS: WeekdayKey[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
	const DAY_LABELS: Record<WeekdayKey, string> = {
		mon: 'Mon', tue: 'Tue', wed: 'Wed', thu: 'Thu', fri: 'Fri', sat: 'Sat', sun: 'Sun'
	};
	let newSchedDays = $state<WeekdayKey[]>([]);
	let newSchedStart = $state('22:00');
	let newSchedEnd = $state('07:00');
	let newSchedFocusId = $state('');

	// Inline edit state for a schedule
	let editingScheduleId = $state<string | null>(null);
	let editSchedDays = $state<WeekdayKey[]>([]);
	let editSchedStart = $state('22:00');
	let editSchedEnd = $state('07:00');
	let editSchedFocusId = $state('');

	// Import / Export (3H)
	let exportJson = $state('');
	let exportCharCount = $derived(exportJson.length);
	let importJson = $state('');
	let importError = $state('');
	let importSuccess = $state('');
	let importBusy = $state(false);

	// Custom CSS theming (3I)
	let customCssDraft = $state('');
	let customCssError = $state('');
	let customCssSuccess = $state('');
	let customCssBusy = $state(false);

	// Manual override: if user clicked a focus in this session, don't auto-switch
	// until the scheduled focus naturally changes to something different.
	let manualOverrideFocusId = $state<string | null | undefined>(undefined);
	// undefined = not overridden (initial / cleared); string | null = manual pick

	// Per-focus filter editing (expanded focus id)
	let focusRulesEditing = $state<string | null>(null);
	// Scratch copy of the rules being edited for a focus
	let focusRulesDraft = $state<Rules>({ default_mode: 'allow', apps: {}, filters: [] });
	// Add-filter form for focus rule editing
	let focusNewFilterField = $state<Filter['field']>('any');
	let focusNewFilterContains = $state('');
	let focusNewFilterAction = $state<Filter['action']>('block');

	// ---------------------------------------------------------------------------
	// Lifecycle
	// ---------------------------------------------------------------------------

	let unlisten: UnlistenFn | null = null;
	let unlistenTrayNav: UnlistenFn | null = null;
	let healthInterval: ReturnType<typeof setInterval> | null = null;
	let scheduleInterval: ReturnType<typeof setInterval> | null = null;

	/** Inject or update the custom CSS <style> tag in the document head. */
	function applyCustomCss(css: string) {
		let tag = document.getElementById('custom-css') as HTMLStyleElement | null;
		if (!tag) {
			tag = document.createElement('style');
			tag.id = 'custom-css';
			document.head.appendChild(tag);
		}
		tag.textContent = css;
	}

	onMount(async () => {
		// Inject saved custom CSS immediately on mount.
		try {
			const css = await invoke<string>('get_custom_css');
			customCssDraft = css;
			applyCustomCss(css);
		} catch (e) {
			console.error('get_custom_css failed:', e);
		}

		// Listen for incoming notifications
		unlisten = await listen<NotificationItem>('notification', (event) => {
			notifications = [event.payload, ...notifications].slice(0, 100);
		});

		// Listen for tray navigation events from the backend
		unlistenTrayNav = await listen<{ panel: string }>('tray-navigate', (event) => {
			const { panel } = event.payload;
			if (panel === 'settings') {
				activePanel = 'settings';
				onSettingsTabActivated();
			} else if (panel === 'history') {
				activateHistory();
			} else if (panel === 'notifications') {
				activePanel = 'notifications';
			}
		});

		// Load initial producer list
		await refreshProducers();

		// Load focuses and schedules
		await refreshFocuses();
		await refreshSchedules();

		// Run one immediate schedule evaluation
		await evalSchedule();

		// Poll health every 3 s
		healthInterval = setInterval(async () => {
			await pollHealth();
		}, 3000);
		await pollHealth();

		// Evaluate schedule every 60 s
		scheduleInterval = setInterval(async () => {
			await evalSchedule();
		}, 60_000);
	});

	onDestroy(() => {
		unlisten?.();
		unlistenTrayNav?.();
		if (healthInterval) clearInterval(healthInterval);
		if (scheduleInterval) clearInterval(scheduleInterval);
	});

	// ---------------------------------------------------------------------------
	// Data helpers
	// ---------------------------------------------------------------------------

	async function refreshProducers() {
		try {
			producers = await invoke<ProducerEntry[]>('get_producers');
		} catch (e) {
			console.error('get_producers failed:', e);
		}
	}

	async function pollHealth() {
		try {
			statuses = await invoke<ProducerStatus[]>('get_health');
		} catch {
			// Silently ignore transient errors
		}
	}

	function statusFor(url: string): ProducerStatus | undefined {
		return statuses.find((s) => s.url === url);
	}

	async function refreshFilters() {
		try {
			rules = await invoke<Rules>('get_rules');
		} catch (e) {
			console.error('get_rules failed:', e);
		}
	}

	async function refreshSeenApps() {
		try {
			seenApps = await invoke<string[]>('get_seen_apps');
		} catch (e) {
			console.error('get_seen_apps failed:', e);
		}
	}

	async function refreshRetention() {
		try {
			retention = await invoke<RetentionConfig>('get_retention');
			retentionDefaultDaysInput = retention.default_days;
			retentionMaxCountEnabled = retention.max_count !== null;
			retentionMaxCountInput = retention.max_count ?? 1000;
		} catch (e) {
			console.error('get_retention failed:', e);
		}
	}

	async function refreshFocuses() {
		try {
			focuses = await invoke<Focus[]>('get_focuses');
			activeFocusId = await invoke<string | null>('get_active_focus');
		} catch (e) {
			console.error('get_focuses failed:', e);
		}
	}

	async function refreshSchedules() {
		try {
			schedules = await invoke<FocusSchedule[]>('get_schedules');
		} catch (e) {
			console.error('get_schedules failed:', e);
		}
	}

	// ---------------------------------------------------------------------------
	// Schedule evaluation helpers
	// ---------------------------------------------------------------------------

	function currentWeekdayKey(): WeekdayKey {
		const day = new Date().getDay(); // 0=Sun…6=Sat
		const map: WeekdayKey[] = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
		return map[day];
	}

	function currentHhmm(): number {
		const d = new Date();
		return d.getHours() * 100 + d.getMinutes();
	}

	/** Format a HHMM integer as "HH:MM" for display. */
	function formatHhmm(hhmm: number): string {
		const h = Math.floor(hhmm / 100).toString().padStart(2, '0');
		const m = (hhmm % 100).toString().padStart(2, '0');
		return `${h}:${m}`;
	}

	/** Parse "HH:MM" to a HHMM integer. Returns NaN on bad input. */
	function parseHhmm(s: string): number {
		const parts = s.split(':');
		if (parts.length !== 2) return NaN;
		const h = parseInt(parts[0], 10);
		const m = parseInt(parts[1], 10);
		if (isNaN(h) || isNaN(m) || h < 0 || h > 23 || m < 0 || m > 59) return NaN;
		return h * 100 + m;
	}

	/** Evaluate schedules via the backend and auto-switch focus if appropriate. */
	async function evalSchedule() {
		try {
			const weekday = currentWeekdayKey();
			const hhmm = currentHhmm();
			const scheduledId = await invoke<string | null>('get_scheduled_focus', {
				weekday,
				hhmm
			});

			// If the user has manually overridden, only clear the override when the
			// scheduled focus transitions away from what they picked.
			if (manualOverrideFocusId !== undefined) {
				if (scheduledId !== manualOverrideFocusId) {
					// Natural transition — clear override and apply new schedule
					manualOverrideFocusId = undefined;
				} else {
					// Still inside the user's override window — don't re-set
					return;
				}
			}

			// Apply scheduled focus if it differs from current
			if (scheduledId !== activeFocusId) {
				await invoke('set_active_focus', { id: scheduledId });
				activeFocusId = scheduledId;
			}
		} catch (e) {
			console.error('schedule eval failed:', e);
		}
	}

	// Called when settings tab is opened so data is fresh
	async function onSettingsTabActivated() {
		if (settingsTab === 'filters') {
			await refreshFilters();
			await refreshSeenApps();
		} else if (settingsTab === 'retention') {
			await refreshRetention();
		} else if (settingsTab === 'focuses') {
			await refreshFocuses();
			await refreshSeenApps();
			await refreshSchedules();
		}
	}

	// ---------------------------------------------------------------------------
	// Focus matching (TS reimplementation — avoids round-trips)
	// ---------------------------------------------------------------------------

	function matchesFilter(f: Filter, n: NotificationItem): boolean {
		const val =
			f.field === 'title'
				? n.title
				: f.field === 'body'
					? n.body
					: f.field === 'appname'
						? n.app_name
						: `${n.title} ${n.body} ${n.app_name}`; // 'any'
		return val.toLowerCase().includes(f.contains.toLowerCase());
	}

	function matchesFocus(n: NotificationItem, focus: Focus | null): boolean {
		if (!focus) return true; // "All"
		const r = focus.rules;
		// 1. App gate
		const appRule = r.apps[n.app_name];
		if (appRule === false) return false;
		if (appRule === true) {
			/* pass app gate */
		} else if (r.default_mode === 'block') return false;
		// 2. Block filters — any match suppresses
		for (const f of r.filters) {
			if (f.action === 'block' && matchesFilter(f, n)) return false;
		}
		// 3. Allow filters — if any exist, at least one must match
		const allowFilters = r.filters.filter((f) => f.action === 'allow');
		if (allowFilters.length > 0 && !allowFilters.some((f) => matchesFilter(f, n))) return false;
		return true;
	}

	// Adapt HistoryItem to the shape matchesFocus expects
	function historyToNotif(h: HistoryItem): NotificationItem {
		return { title: h.title, body: h.body, app_name: h.app_name, timestamp_ms: 0 };
	}

	// The active Focus object (null = "All")
	let activeFocus = $derived(focuses.find((f) => f.id === activeFocusId) ?? null);

	// Filtered views
	let visibleNotifications = $derived(
		notifications.filter((n) => matchesFocus(n, activeFocus))
	);
	let visibleHistory = $derived(
		historyItems.filter((h) => matchesFocus(historyToNotif(h), activeFocus))
	);

	// ---------------------------------------------------------------------------
	// History actions
	// ---------------------------------------------------------------------------

	async function loadHistory(reset: boolean) {
		if (historyLoading) return;
		historyLoading = true;
		historyError = '';
		const offset = reset ? 0 : historyOffset;
		try {
			const items = await invoke<HistoryItem[]>('get_history', {
				appName: historyFilter.trim() || null,
				limit: HISTORY_PAGE_SIZE,
				offset
			});
			if (reset) {
				historyItems = items;
			} else {
				historyItems = [...historyItems, ...items];
			}
			historyOffset = offset + items.length;
			historyHasMore = items.length === HISTORY_PAGE_SIZE;
		} catch (e) {
			historyError = String(e);
		} finally {
			historyLoading = false;
		}
	}

	async function handleHistoryFilterChange() {
		historyOffset = 0;
		await loadHistory(true);
	}

	async function handleLoadMore() {
		await loadHistory(false);
	}

	async function handlePruneNow() {
		pruneLoading = true;
		pruneResult = null;
		try {
			const deleted = await invoke<number>('prune_now');
			pruneResult = deleted === 0 ? 'Nothing to prune.' : `Pruned ${deleted} row${deleted !== 1 ? 's' : ''}.`;
			// Reload history after prune
			historyOffset = 0;
			await loadHistory(true);
		} catch (e) {
			pruneResult = `Error: ${e}`;
		} finally {
			pruneLoading = false;
		}
	}

	// Trigger history load when switching to the history panel
	function activateHistory() {
		activePanel = 'history';
		if (historyItems.length === 0) {
			loadHistory(true);
		}
	}

	// ---------------------------------------------------------------------------
	// Settings actions — producers
	// ---------------------------------------------------------------------------

	async function handleAdd() {
		const url = newUrl.trim();
		if (!url) return;
		addError = '';
		addBusy = true;
		try {
			await invoke('add_producer', {
				url,
				label: newLabel.trim() || null
			});
			newUrl = '';
			newLabel = '';
			await refreshProducers();
		} catch (e) {
			addError = String(e);
		} finally {
			addBusy = false;
		}
	}

	async function handleRemove(url: string) {
		try {
			await invoke('remove_producer', { url });
			await refreshProducers();
		} catch (e) {
			console.error('remove_producer failed:', e);
		}
	}

	async function handleToggle(url: string, enabled: boolean) {
		try {
			await invoke('set_producer_enabled', { url, enabled });
			await refreshProducers();
			await pollHealth();
		} catch (e) {
			console.error('set_producer_enabled failed:', e);
		}
	}

	// ---------------------------------------------------------------------------
	// Settings actions — filters
	// ---------------------------------------------------------------------------

	async function handleSetDefaultMode(mode: 'allow' | 'block') {
		filtersError = '';
		filtersBusy = true;
		try {
			await invoke('set_default_mode', { mode });
			await refreshFilters();
		} catch (e) {
			filtersError = String(e);
		} finally {
			filtersBusy = false;
		}
	}

	// Three-way app rule: 'allow' | 'default' | 'block'
	type AppRuleChoice = 'allow' | 'default' | 'block';

	function appRuleFor(app_name: string): AppRuleChoice {
		const v = rules.apps[app_name];
		if (v === true) return 'allow';
		if (v === false) return 'block';
		return 'default';
	}

	async function handleAppRule(app_name: string, choice: AppRuleChoice) {
		filtersError = '';
		try {
			if (choice === 'default') {
				await invoke('remove_app_rule', { app_name });
			} else {
				await invoke('set_app_rule', { app_name, enabled: choice === 'allow' });
			}
			await refreshFilters();
		} catch (e) {
			filtersError = String(e);
		}
	}

	async function handleAddFilter() {
		const contains = newFilterContains.trim();
		if (!contains) return;
		filtersError = '';
		filtersBusy = true;
		try {
			await invoke('add_filter', {
				field: newFilterField,
				contains,
				action: newFilterAction
			});
			newFilterContains = '';
			await refreshFilters();
		} catch (e) {
			filtersError = String(e);
		} finally {
			filtersBusy = false;
		}
	}

	async function handleRemoveFilter(index: number) {
		filtersError = '';
		try {
			await invoke('remove_filter', { index });
			await refreshFilters();
		} catch (e) {
			filtersError = String(e);
		}
	}

	// All app names to show: union of seenApps + existing rules.apps keys
	let allAppNames = $derived(
		[...new Set([...seenApps, ...Object.keys(rules.apps)])].sort()
	);

	// Field display labels
	const fieldLabels: Record<Filter['field'], string> = {
		title: 'Title',
		body: 'Body',
		appname: 'App Name',
		any: 'Any'
	};

	// ---------------------------------------------------------------------------
	// Settings actions — retention
	// ---------------------------------------------------------------------------

	async function handleSaveDefaultDays() {
		retentionError = '';
		retentionBusy = true;
		try {
			await invoke('set_retention_default', { days: retentionDefaultDaysInput });
			await refreshRetention();
		} catch (e) {
			retentionError = String(e);
		} finally {
			retentionBusy = false;
		}
	}

	async function handleSaveMaxCount() {
		retentionError = '';
		retentionBusy = true;
		try {
			const maxCount = retentionMaxCountEnabled ? retentionMaxCountInput : null;
			await invoke('set_retention_max_count', { maxCount });
			await refreshRetention();
		} catch (e) {
			retentionError = String(e);
		} finally {
			retentionBusy = false;
		}
	}

	async function handleSaveProducerRetention(url: string, days: number) {
		retentionError = '';
		try {
			await invoke('set_retention_producer', { producerUrl: url, days });
			await refreshRetention();
		} catch (e) {
			retentionError = String(e);
		}
	}

	async function handleRemoveProducerRetention(url: string) {
		retentionError = '';
		try {
			await invoke('remove_retention_producer', { producerUrl: url });
			await refreshRetention();
		} catch (e) {
			retentionError = String(e);
		}
	}

	// Per-producer input state (keyed by URL)
	let producerDaysInputs = $state<Record<string, number>>({});

	// Sync producerDaysInputs when retention loads
	$effect(() => {
		for (const url of Object.keys(retention.per_producer)) {
			if (!(url in producerDaysInputs)) {
				producerDaysInputs[url] = retention.per_producer[url];
			}
		}
	});

	// ---------------------------------------------------------------------------
	// Settings actions — focuses
	// ---------------------------------------------------------------------------

	async function handleAddFocus() {
		const name = newFocusName.trim();
		if (!name) return;
		focusesError = '';
		focusesBusy = true;
		try {
			await invoke<Focus>('add_focus', { name, icon: newFocusIcon.trim() || null });
			newFocusName = '';
			newFocusIcon = '';
			await refreshFocuses();
		} catch (e) {
			focusesError = String(e);
		} finally {
			focusesBusy = false;
		}
	}

	async function handleCloneFocus(id: string) {
		focusesError = '';
		try {
			await invoke<Focus>('clone_focus', { id });
			await refreshFocuses();
		} catch (e) {
			focusesError = String(e);
		}
	}

	async function handleRemoveFocus(id: string) {
		focusesError = '';
		try {
			await invoke('remove_focus', { id });
			await refreshFocuses();
		} catch (e) {
			focusesError = String(e);
		}
	}

	function startEditFocus(focus: Focus) {
		editingFocusId = focus.id;
		editFocusName = focus.name;
		editFocusIcon = focus.icon ?? '';
	}

	function cancelEditFocus() {
		editingFocusId = null;
		editFocusName = '';
		editFocusIcon = '';
	}

	async function saveEditFocus(id: string) {
		const name = editFocusName.trim();
		if (!name) return;
		focusesError = '';
		try {
			await invoke('update_focus', {
				id,
				name,
				icon: editFocusIcon.trim() || ''  // empty string = clear icon
			});
			cancelEditFocus();
			await refreshFocuses();
		} catch (e) {
			focusesError = String(e);
		}
	}

	// Open inline rules editor for a focus
	function openFocusRulesEditor(focus: Focus) {
		focusRulesEditing = focus.id;
		// Deep copy the rules
		focusRulesDraft = JSON.parse(JSON.stringify(focus.rules)) as Rules;
		focusNewFilterField = 'any';
		focusNewFilterContains = '';
		focusNewFilterAction = 'block';
	}

	function closeFocusRulesEditor() {
		focusRulesEditing = null;
	}

	async function saveFocusRules(id: string) {
		focusesError = '';
		try {
			await invoke('set_focus_rules', { id, rules: focusRulesDraft });
			closeFocusRulesEditor();
			await refreshFocuses();
		} catch (e) {
			focusesError = String(e);
		}
	}

	function focusDraftAppRule(app_name: string): AppRuleChoice {
		const v = focusRulesDraft.apps[app_name];
		if (v === true) return 'allow';
		if (v === false) return 'block';
		return 'default';
	}

	function handleFocusDraftAppRule(app_name: string, choice: AppRuleChoice) {
		if (choice === 'default') {
			delete focusRulesDraft.apps[app_name];
			focusRulesDraft = { ...focusRulesDraft };
		} else {
			focusRulesDraft = {
				...focusRulesDraft,
				apps: { ...focusRulesDraft.apps, [app_name]: choice === 'allow' }
			};
		}
	}

	function handleFocusDraftAddFilter() {
		const contains = focusNewFilterContains.trim();
		if (!contains) return;
		focusRulesDraft = {
			...focusRulesDraft,
			filters: [
				...focusRulesDraft.filters,
				{ field: focusNewFilterField, contains, action: focusNewFilterAction }
			]
		};
		focusNewFilterContains = '';
	}

	function handleFocusDraftRemoveFilter(index: number) {
		focusRulesDraft = {
			...focusRulesDraft,
			filters: focusRulesDraft.filters.filter((_, i) => i !== index)
		};
	}

	// All app names for focus rules editor: union of seenApps + existing rule keys
	let allAppNamesForFocus = $derived(
		[...new Set([...seenApps, ...Object.keys(focusRulesDraft.apps)])].sort()
	);

	// ---------------------------------------------------------------------------
	// Focus sidebar actions
	// ---------------------------------------------------------------------------

	async function handleSwitchFocus(id: string | null) {
		try {
			await invoke('set_active_focus', { id });
			activeFocusId = id;
			// Record manual override so auto-scheduler doesn't immediately undo it.
			manualOverrideFocusId = id;
		} catch (e) {
			console.error('set_active_focus failed:', e);
		}
	}

	// ---------------------------------------------------------------------------
	// Settings actions — schedules
	// ---------------------------------------------------------------------------

	async function handleAddSchedule() {
		if (!newSchedFocusId || newSchedDays.length === 0) return;
		const startVal = parseHhmm(newSchedStart);
		const endVal = parseHhmm(newSchedEnd);
		if (isNaN(startVal) || isNaN(endVal)) {
			schedulesError = 'Invalid time format. Use HH:MM.';
			return;
		}
		schedulesError = '';
		schedulesBusy = true;
		try {
			await invoke<FocusSchedule>('add_schedule', {
				focusId: newSchedFocusId,
				days: newSchedDays,
				startHhmm: startVal,
				endHhmm: endVal
			});
			newSchedDays = [];
			newSchedStart = '22:00';
			newSchedEnd = '07:00';
			newSchedFocusId = '';
			await refreshSchedules();
		} catch (e) {
			schedulesError = String(e);
		} finally {
			schedulesBusy = false;
		}
	}

	async function handleRemoveSchedule(id: string) {
		schedulesError = '';
		try {
			await invoke('remove_schedule', { id });
			await refreshSchedules();
		} catch (e) {
			schedulesError = String(e);
		}
	}

	async function handleToggleSchedule(s: FocusSchedule, enabled: boolean) {
		schedulesError = '';
		try {
			await invoke('update_schedule', {
				id: s.id,
				focusId: s.focus_id,
				days: s.days,
				startHhmm: s.time_range.start_hhmm,
				endHhmm: s.time_range.end_hhmm,
				enabled
			});
			await refreshSchedules();
		} catch (e) {
			schedulesError = String(e);
		}
	}

	function startEditSchedule(s: FocusSchedule) {
		editingScheduleId = s.id;
		editSchedDays = [...s.days];
		editSchedStart = formatHhmm(s.time_range.start_hhmm);
		editSchedEnd = formatHhmm(s.time_range.end_hhmm);
		editSchedFocusId = s.focus_id;
	}

	function cancelEditSchedule() {
		editingScheduleId = null;
	}

	async function saveEditSchedule(s: FocusSchedule) {
		const startVal = parseHhmm(editSchedStart);
		const endVal = parseHhmm(editSchedEnd);
		if (isNaN(startVal) || isNaN(endVal)) {
			schedulesError = 'Invalid time format. Use HH:MM.';
			return;
		}
		if (editSchedDays.length === 0) {
			schedulesError = 'Select at least one day.';
			return;
		}
		schedulesError = '';
		try {
			await invoke('update_schedule', {
				id: s.id,
				focusId: editSchedFocusId,
				days: editSchedDays,
				startHhmm: startVal,
				endHhmm: endVal,
				enabled: s.enabled
			});
			cancelEditSchedule();
			await refreshSchedules();
		} catch (e) {
			schedulesError = String(e);
		}
	}

	function toggleEditDay(day: WeekdayKey) {
		if (editSchedDays.includes(day)) {
			editSchedDays = editSchedDays.filter((d) => d !== day);
		} else {
			editSchedDays = [...editSchedDays, day];
		}
	}

	function toggleNewDay(day: WeekdayKey) {
		if (newSchedDays.includes(day)) {
			newSchedDays = newSchedDays.filter((d) => d !== day);
		} else {
			newSchedDays = [...newSchedDays, day];
		}
	}

	function focusNameById(id: string): string {
		return focuses.find((f) => f.id === id)?.name ?? id;
	}

	// ---------------------------------------------------------------------------
	// Import / Export actions (3H)
	// ---------------------------------------------------------------------------

	async function handleExportConfig() {
		try {
			exportJson = await invoke<string>('export_config');
			// Auto-select the textarea content for easy copying (deferred so DOM updates first).
			await Promise.resolve();
			const ta = document.getElementById('export-textarea') as HTMLTextAreaElement | null;
			ta?.select();
		} catch (e) {
			exportJson = `Error: ${e}`;
		}
	}

	async function handleImportConfig() {
		const json = importJson.trim();
		if (!json) return;
		importError = '';
		importSuccess = '';
		importBusy = true;
		try {
			await invoke('import_config', { json });
			importSuccess = 'Config imported successfully.';
			importJson = '';
			// Reload all state to reflect the new config.
			await refreshProducers();
			await refreshFilters();
			await refreshRetention();
			await refreshFocuses();
			await refreshSchedules();
		} catch (e) {
			importError = String(e);
		} finally {
			importBusy = false;
		}
	}

	// ---------------------------------------------------------------------------
	// Custom CSS actions (3I)
	// ---------------------------------------------------------------------------

	async function handleApplyCss() {
		customCssError = '';
		customCssSuccess = '';
		customCssBusy = true;
		try {
			await invoke('set_custom_css', { css: customCssDraft });
			applyCustomCss(customCssDraft);
			customCssSuccess = 'CSS applied.';
		} catch (e) {
			customCssError = String(e);
		} finally {
			customCssBusy = false;
		}
	}

	async function handleResetCss() {
		customCssError = '';
		customCssSuccess = '';
		customCssBusy = true;
		try {
			await invoke('set_custom_css', { css: '' });
			customCssDraft = '';
			applyCustomCss('');
			customCssSuccess = 'CSS cleared.';
		} catch (e) {
			customCssError = String(e);
		} finally {
			customCssBusy = false;
		}
	}

	// Sorted focuses for sidebar display
	let sortedFocuses = $derived([...focuses].sort((a, b) => a.sort_order - b.sort_order));

	// Label for the active focus
	let activeFocusLabel = $derived(
		activeFocusId === null ? 'All' : (focuses.find((f) => f.id === activeFocusId)?.name ?? 'All')
	);

	// ---------------------------------------------------------------------------
	// Helpers
	// ---------------------------------------------------------------------------

	function dotFor(status: ProducerStatus | undefined, enabled: boolean): string {
		if (!enabled) return '⚫';
		if (!status) return '⚪';
		switch (status.state) {
			case 'connected': return '🟢';
			case 'reconnecting': return '🟡';
			case 'connecting': return '🟡';
			case 'unreachable': return '🔴';
		}
	}

	function labelFor(entry: ProducerEntry): string {
		return entry.label ?? entry.url;
	}

	function relativeTime(ms: number): string {
		const diff = Math.floor((Date.now() - ms) / 1000);
		if (diff < 5) return 'just now';
		if (diff < 60) return `${diff}s ago`;
		if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
		return `${Math.floor(diff / 3600)}h ago`;
	}

	function relativeTimeFromIso(iso: string): string {
		const ms = new Date(iso).getTime();
		if (isNaN(ms)) return iso;
		return relativeTime(ms);
	}
</script>

<div class="app-shell">
	<!-- Focuses sidebar -->
	<aside class="focuses-sidebar">
		<div class="focuses-brand">notifwire</div>
		<div class="focuses-label">Focus</div>

		<!-- Built-in "All" focus -->
		<button
			class="focus-item"
			class:active={activeFocusId === null}
			onclick={() => handleSwitchFocus(null)}
		>
			<span class="focus-icon">✦</span>
			<span class="focus-name">All</span>
		</button>

		<!-- User focuses -->
		{#each sortedFocuses as f (f.id)}
			<button
				class="focus-item"
				class:active={activeFocusId === f.id}
				onclick={() => handleSwitchFocus(f.id)}
			>
				{#if f.icon}
					<span class="focus-icon">{f.icon}</span>
				{:else}
					<span class="focus-icon focus-icon-empty">○</span>
				{/if}
				<span class="focus-name">{f.name}</span>
			</button>
		{/each}

		<div class="focuses-footer">
			<button
				class="btn-add-focus"
				title="Manage focuses"
				onclick={() => { activePanel = 'settings'; settingsTab = 'focuses'; onSettingsTabActivated(); }}
			>
				+ Focuses
			</button>
		</div>
	</aside>

	<!-- Nav sidebar -->
	<nav class="sidebar">
		<button
			class="nav-item"
			class:active={activePanel === 'notifications'}
			onclick={() => { activePanel = 'notifications'; }}
		>
			Notifications
		</button>
		<button
			class="nav-item"
			class:active={activePanel === 'history'}
			onclick={activateHistory}
		>
			History
		</button>
		<button
			class="nav-item"
			class:active={activePanel === 'settings'}
			onclick={async () => { activePanel = 'settings'; await onSettingsTabActivated(); }}
		>
			Settings
		</button>
	</nav>

	<!-- Right content -->
	<main class="content">

		<!-- Notifications panel -->
		{#if activePanel === 'notifications'}
			<section class="panel">
				<h2 class="panel-title">
					Notifications
					{#if activeFocusId !== null}
						<span class="focus-badge">{activeFocusLabel}</span>
					{/if}
				</h2>
				{#if visibleNotifications.length === 0}
					<p class="empty">
						{notifications.length === 0
							? 'No notifications yet.'
							: 'No notifications match the active focus.'}
					</p>
				{:else}
					<ul class="notif-list">
						{#each visibleNotifications as n (n.timestamp_ms + n.title)}
							<li class="notification-item">
								<div class="notif-header">
									<span class="app-name">{n.app_name}</span>
									<span class="notif-title">{n.title}</span>
									<span class="timestamp">{relativeTime(n.timestamp_ms)}</span>
								</div>
								{#if n.body}
									<p class="notif-body">{n.body}</p>
								{/if}
							</li>
						{/each}
					</ul>
				{/if}
			</section>

		<!-- History panel -->
		{:else if activePanel === 'history'}
			<section class="panel">
				<h2 class="panel-title">
					History
					{#if activeFocusId !== null}
						<span class="focus-badge">{activeFocusLabel}</span>
					{/if}
				</h2>

				<div class="history-toolbar">
					<input
						type="text"
						bind:value={historyFilter}
						placeholder="Filter by app name…"
						class="history-filter-input"
						oninput={handleHistoryFilterChange}
					/>
					<button
						class="btn-prune"
						onclick={handlePruneNow}
						disabled={pruneLoading}
					>
						{pruneLoading ? 'Pruning…' : 'Clear older than retention'}
					</button>
				</div>

				{#if pruneResult}
					<p class="prune-result">{pruneResult}</p>
				{/if}

				{#if historyError}
					<p class="add-error">{historyError}</p>
				{/if}

				{#if historyLoading && historyItems.length === 0}
					<p class="empty">Loading…</p>
				{:else if visibleHistory.length === 0}
					<p class="empty">
						{historyItems.length === 0
							? 'No notifications in history.'
							: 'No history matches the active focus.'}
					</p>
				{:else}
					<ul class="notif-list">
						{#each visibleHistory as n (n.id)}
							<li class="notification-item">
								<div class="notif-header">
									<span class="app-name">{n.app_name}</span>
									<span class="notif-title">{n.title}</span>
									<span class="timestamp">{relativeTimeFromIso(n.timestamp)}</span>
								</div>
								{#if n.body}
									<p class="notif-body">{n.body}</p>
								{/if}
								<p class="notif-meta">from {n.producer_node}</p>
							</li>
						{/each}
					</ul>

					{#if historyHasMore}
						<button
							class="btn-load-more"
							onclick={handleLoadMore}
							disabled={historyLoading}
						>
							{historyLoading ? 'Loading…' : 'Load more'}
						</button>
					{/if}
				{/if}
			</section>

		<!-- Settings panel -->
		{:else}
			<section class="panel">
				<h2 class="panel-title">Settings</h2>

				<!-- Settings tab row -->
				<div class="settings-tabs">
					<button
						class="settings-tab"
						class:active={settingsTab === 'producers'}
						onclick={() => { settingsTab = 'producers'; }}
					>
						Producers
					</button>
					<button
						class="settings-tab"
						class:active={settingsTab === 'filters'}
						onclick={async () => { settingsTab = 'filters'; await refreshFilters(); await refreshSeenApps(); }}
					>
						Filters
					</button>
					<button
						class="settings-tab"
						class:active={settingsTab === 'retention'}
						onclick={async () => { settingsTab = 'retention'; await refreshRetention(); await refreshProducers(); }}
					>
						Retention
					</button>
					<button
						class="settings-tab"
						class:active={settingsTab === 'focuses'}
						onclick={async () => { settingsTab = 'focuses'; await refreshFocuses(); await refreshSeenApps(); }}
					>
						Focuses
					</button>
					<button
						class="settings-tab"
						class:active={settingsTab === 'import-export'}
						onclick={() => { settingsTab = 'import-export'; exportJson = ''; importJson = ''; importError = ''; importSuccess = ''; }}
					>
						Import / Export
					</button>
					<button
						class="settings-tab"
						class:active={settingsTab === 'theme'}
						onclick={() => { settingsTab = 'theme'; }}
					>
						Theme
					</button>
				</div>

				<!-- Producers sub-panel -->
				{#if settingsTab === 'producers'}
					{#if producers.length === 0}
						<p class="empty">No producers configured.</p>
					{:else}
						<ul class="producer-list">
							{#each producers as entry (entry.url)}
								{@const status = statusFor(entry.url)}
								<li class="producer-row">
									<span class="status-dot" title={status?.state ?? (entry.enabled ? 'connecting' : 'disabled')}>
										{dotFor(status, entry.enabled)}
									</span>
									<div class="producer-info">
										<span class="producer-label">{labelFor(entry)}</span>
										{#if entry.label}
											<span class="producer-url">{entry.url}</span>
										{/if}
										{#if status?.last_error && entry.enabled}
											<span class="producer-error">{status.last_error}</span>
										{/if}
									</div>
									<label class="toggle" title={entry.enabled ? 'Disable' : 'Enable'}>
										<input
											type="checkbox"
											checked={entry.enabled}
											onchange={(e) => handleToggle(entry.url, (e.target as HTMLInputElement).checked)}
										/>
										<span class="toggle-track"></span>
									</label>
									<button class="btn-remove" onclick={() => handleRemove(entry.url)} title="Remove">
										✕
									</button>
								</li>
							{/each}
						</ul>
					{/if}

					<!-- Add-producer form -->
					<div class="add-form">
						<h3 class="add-title">Add producer</h3>
						<div class="add-fields">
							<input
								type="text"
								bind:value={newUrl}
								placeholder="http://localhost:8765"
								class="field-url"
							/>
							<input
								type="text"
								bind:value={newLabel}
								placeholder="Label (optional)"
								class="field-label"
							/>
							<button
								class="btn-add"
								onclick={handleAdd}
								disabled={addBusy || !newUrl.trim()}
							>
								{addBusy ? 'Adding…' : 'Add'}
							</button>
						</div>
						{#if addError}
							<p class="add-error">{addError}</p>
						{/if}
					</div>

				<!-- Filters sub-panel -->
				{:else if settingsTab === 'filters'}
					<div class="filters-panel">

						<!-- Default mode -->
						<div class="filters-section">
							<h3 class="section-title">Default mode</h3>
							<div class="mode-row">
								<label class="mode-option">
									<input
										type="radio"
										name="default_mode"
										value="allow"
										checked={rules.default_mode === 'allow'}
										onchange={() => handleSetDefaultMode('allow')}
										disabled={filtersBusy}
									/>
									<span>Allow all by default</span>
								</label>
								<label class="mode-option">
									<input
										type="radio"
										name="default_mode"
										value="block"
										checked={rules.default_mode === 'block'}
										onchange={() => handleSetDefaultMode('block')}
										disabled={filtersBusy}
									/>
									<span>Block all by default</span>
								</label>
							</div>
						</div>

						<!-- Per-app rules -->
						<div class="filters-section">
							<div class="section-header">
								<h3 class="section-title">Apps</h3>
								<button class="btn-refresh" onclick={async () => { await refreshSeenApps(); await refreshFilters(); }}>
									Refresh
								</button>
							</div>
							{#if allAppNames.length === 0}
								<p class="empty">No apps seen yet. Receive a notification to populate this list.</p>
							{:else}
								<ul class="app-list">
									{#each allAppNames as name (name)}
										{@const choice = appRuleFor(name)}
										<li class="app-row">
											<span class="app-row-name">{name}</span>
											<div class="app-rule-buttons">
												<button
													class="rule-btn"
													class:active-allow={choice === 'allow'}
													onclick={() => handleAppRule(name, 'allow')}
												>Allow</button>
												<button
													class="rule-btn"
													class:active-default={choice === 'default'}
													onclick={() => handleAppRule(name, 'default')}
												>Default</button>
												<button
													class="rule-btn"
													class:active-block={choice === 'block'}
													onclick={() => handleAppRule(name, 'block')}
												>Block</button>
											</div>
										</li>
									{/each}
								</ul>
							{/if}
						</div>

						<!-- Keyword filters -->
						<div class="filters-section">
							<h3 class="section-title">Keyword filters</h3>
							{#if rules.filters.length === 0}
								<p class="empty">No keyword filters.</p>
							{:else}
								<ul class="kw-list">
									{#each rules.filters as f, i (i)}
										<li class="kw-row">
											<span class="badge field-badge">{fieldLabels[f.field]}</span>
											<span class="kw-contains">contains</span>
											<span class="kw-keyword">"{f.contains}"</span>
											<span class="badge" class:badge-allow={f.action === 'allow'} class:badge-block={f.action === 'block'}>
												{f.action}
											</span>
											<button class="btn-remove" onclick={() => handleRemoveFilter(i)} title="Remove">
												✕
											</button>
										</li>
									{/each}
								</ul>
							{/if}

							<!-- Add filter form -->
							<div class="kw-add-form">
								<h4 class="add-title">Add filter</h4>
								<div class="kw-add-fields">
									<select bind:value={newFilterField} class="field-select">
										<option value="title">Title</option>
										<option value="body">Body</option>
										<option value="appname">App Name</option>
										<option value="any">Any</option>
									</select>
									<input
										type="text"
										bind:value={newFilterContains}
										placeholder="keyword"
										class="field-keyword"
									/>
									<select bind:value={newFilterAction} class="field-select">
										<option value="block">Block</option>
										<option value="allow">Allow</option>
									</select>
									<button
										class="btn-add"
										onclick={handleAddFilter}
										disabled={filtersBusy || !newFilterContains.trim()}
									>
										{filtersBusy ? 'Adding…' : 'Add'}
									</button>
								</div>
							</div>
						</div>

						{#if filtersError}
							<p class="add-error">{filtersError}</p>
						{/if}

					</div>

				<!-- Retention sub-panel -->
				{:else if settingsTab === 'retention'}
					<div class="filters-panel">

						<!-- Global default -->
						<div class="filters-section">
							<h3 class="section-title">Global default</h3>
							<div class="retention-row">
								<label class="retention-label">Keep notifications for</label>
								<input
									type="number"
									min="1"
									max="3650"
									bind:value={retentionDefaultDaysInput}
									class="retention-days-input"
									disabled={retentionBusy}
								/>
								<span class="retention-unit">days</span>
								<button
									class="btn-add"
									onclick={handleSaveDefaultDays}
									disabled={retentionBusy}
								>
									Save
								</button>
							</div>
						</div>

						<!-- Max count cap -->
						<div class="filters-section">
							<h3 class="section-title">Max notification count</h3>
							<div class="retention-row">
								<label class="mode-option">
									<input
										type="checkbox"
										bind:checked={retentionMaxCountEnabled}
									/>
									<span>Enforce a cap of</span>
								</label>
								<input
									type="number"
									min="1"
									bind:value={retentionMaxCountInput}
									class="retention-days-input"
									disabled={!retentionMaxCountEnabled || retentionBusy}
								/>
								<span class="retention-unit">notifications</span>
								<button
									class="btn-add"
									onclick={handleSaveMaxCount}
									disabled={retentionBusy}
								>
									Save
								</button>
							</div>
						</div>

						<!-- Per-producer overrides -->
						<div class="filters-section">
							<h3 class="section-title">Per-producer overrides</h3>
							{#if producers.length === 0}
								<p class="empty">No producers configured.</p>
							{:else}
								<ul class="app-list">
									{#each producers as entry (entry.url)}
										{@const override_days = retention.per_producer[entry.url]}
										<li class="app-row retention-producer-row">
											<span class="app-row-name">{entry.label ?? entry.url}</span>
											<input
												type="number"
												min="1"
												max="3650"
												value={producerDaysInputs[entry.url] ?? override_days ?? retention.default_days}
												class="retention-days-input-sm"
												oninput={(e) => {
													producerDaysInputs[entry.url] = parseInt((e.target as HTMLInputElement).value, 10);
												}}
											/>
											<span class="retention-unit">days</span>
											<button
												class="btn-add btn-sm"
												onclick={() => handleSaveProducerRetention(entry.url, producerDaysInputs[entry.url] ?? retention.default_days)}
											>
												Save
											</button>
											{#if override_days !== undefined}
												<button
													class="btn-remove"
													onclick={() => handleRemoveProducerRetention(entry.url)}
													title="Reset to default"
												>
													✕
												</button>
											{/if}
										</li>
									{/each}
								</ul>
							{/if}
						</div>

						{#if retentionError}
							<p class="add-error">{retentionError}</p>
						{/if}

					</div>

				<!-- Focuses sub-panel -->
				{:else if settingsTab === 'focuses'}
					<div class="filters-panel">

						<div class="filters-section">
							<h3 class="section-title">Focuses</h3>
							<p class="section-hint">Focuses are named filter profiles. Switch between them in the left sidebar. "All" always shows everything.</p>

							{#if sortedFocuses.length === 0}
								<p class="empty">No custom focuses yet.</p>
							{:else}
								<ul class="focus-list">
									{#each sortedFocuses as focus (focus.id)}
										<li class="focus-list-item">
											{#if editingFocusId === focus.id}
												<!-- Inline edit row -->
												<div class="focus-edit-row">
													<input
														type="text"
														bind:value={editFocusIcon}
														placeholder="icon (emoji)"
														class="field-icon"
														maxlength="4"
													/>
													<input
														type="text"
														bind:value={editFocusName}
														placeholder="Focus name"
														class="field-focus-name"
													/>
													<button class="btn-add btn-sm" onclick={() => saveEditFocus(focus.id)}>Save</button>
													<button class="btn-cancel btn-sm" onclick={cancelEditFocus}>Cancel</button>
												</div>
											{:else}
												<!-- Display row -->
												<div class="focus-display-row">
													<span class="focus-list-icon">{focus.icon ?? '○'}</span>
													<span class="focus-list-name">{focus.name}</span>
													{#if activeFocusId === focus.id}
														<span class="badge badge-active">active</span>
													{/if}
													<div class="focus-actions">
														<button class="btn-action" onclick={() => startEditFocus(focus)} title="Rename">✎</button>
														<button class="btn-action" onclick={() => {
															if (focusRulesEditing === focus.id) {
																closeFocusRulesEditor();
															} else {
																openFocusRulesEditor(focus);
															}
														}} title="Edit filters">
															{focusRulesEditing === focus.id ? '▲' : '▼'} Filters
														</button>
														<button class="btn-action" onclick={() => handleCloneFocus(focus.id)} title="Clone">⎘</button>
														<button class="btn-remove" onclick={() => handleRemoveFocus(focus.id)} title="Delete">✕</button>
													</div>
												</div>
											{/if}

											<!-- Inline rules editor for this focus -->
											{#if focusRulesEditing === focus.id}
												<div class="focus-rules-editor">
													<!-- Default mode -->
													<div class="focus-rules-section">
														<span class="focus-rules-label">Default mode</span>
														<div class="mode-row">
															<label class="mode-option">
																<input
																	type="radio"
																	name="focus_default_mode_{focus.id}"
																	value="allow"
																	checked={focusRulesDraft.default_mode === 'allow'}
																	onchange={() => { focusRulesDraft = { ...focusRulesDraft, default_mode: 'allow' }; }}
																/>
																<span>Allow all</span>
															</label>
															<label class="mode-option">
																<input
																	type="radio"
																	name="focus_default_mode_{focus.id}"
																	value="block"
																	checked={focusRulesDraft.default_mode === 'block'}
																	onchange={() => { focusRulesDraft = { ...focusRulesDraft, default_mode: 'block' }; }}
																/>
																<span>Block all</span>
															</label>
														</div>
													</div>

													<!-- Per-app rules -->
													{#if allAppNamesForFocus.length > 0}
														<div class="focus-rules-section">
															<span class="focus-rules-label">Apps</span>
															<ul class="app-list">
																{#each allAppNamesForFocus as name (name)}
																	{@const choice = focusDraftAppRule(name)}
																	<li class="app-row">
																		<span class="app-row-name">{name}</span>
																		<div class="app-rule-buttons">
																			<button
																				class="rule-btn"
																				class:active-allow={choice === 'allow'}
																				onclick={() => handleFocusDraftAppRule(name, 'allow')}
																			>Allow</button>
																			<button
																				class="rule-btn"
																				class:active-default={choice === 'default'}
																				onclick={() => handleFocusDraftAppRule(name, 'default')}
																			>Default</button>
																			<button
																				class="rule-btn"
																				class:active-block={choice === 'block'}
																				onclick={() => handleFocusDraftAppRule(name, 'block')}
																			>Block</button>
																		</div>
																	</li>
																{/each}
															</ul>
														</div>
													{/if}

													<!-- Keyword filters -->
													<div class="focus-rules-section">
														<span class="focus-rules-label">Keyword filters</span>
														{#if focusRulesDraft.filters.length === 0}
															<p class="empty" style="margin:0;">No keyword filters.</p>
														{:else}
															<ul class="kw-list">
																{#each focusRulesDraft.filters as f, i (i)}
																	<li class="kw-row">
																		<span class="badge field-badge">{fieldLabels[f.field]}</span>
																		<span class="kw-contains">contains</span>
																		<span class="kw-keyword">"{f.contains}"</span>
																		<span class="badge" class:badge-allow={f.action === 'allow'} class:badge-block={f.action === 'block'}>
																			{f.action}
																		</span>
																		<button class="btn-remove" onclick={() => handleFocusDraftRemoveFilter(i)} title="Remove">✕</button>
																	</li>
																{/each}
															</ul>
														{/if}
														<div class="kw-add-fields" style="margin-top:0.4rem;">
															<select bind:value={focusNewFilterField} class="field-select">
																<option value="title">Title</option>
																<option value="body">Body</option>
																<option value="appname">App Name</option>
																<option value="any">Any</option>
															</select>
															<input
																type="text"
																bind:value={focusNewFilterContains}
																placeholder="keyword"
																class="field-keyword"
															/>
															<select bind:value={focusNewFilterAction} class="field-select">
																<option value="block">Block</option>
																<option value="allow">Allow</option>
															</select>
															<button
																class="btn-add btn-sm"
																onclick={handleFocusDraftAddFilter}
																disabled={!focusNewFilterContains.trim()}
															>Add</button>
														</div>
													</div>

													<div class="focus-rules-footer">
														<button class="btn-add btn-sm" onclick={() => saveFocusRules(focus.id)}>Save rules</button>
														<button class="btn-cancel btn-sm" onclick={closeFocusRulesEditor}>Discard</button>
													</div>
												</div>
											{/if}
										</li>
									{/each}
								</ul>
							{/if}
						</div>

						<!-- Schedules section -->
						<div class="filters-section">
							<h3 class="section-title">Schedules</h3>
							<p class="section-hint">Automatically activate a focus during set time windows. The scheduler runs every minute; a manual focus switch pauses it until the next transition.</p>

							{#if schedules.length === 0}
								<p class="empty">No schedules yet.</p>
							{:else}
								<ul class="schedule-list">
									{#each schedules as sched (sched.id)}
										<li class="schedule-item" class:sched-disabled={!sched.enabled}>
											{#if editingScheduleId === sched.id}
												<!-- Inline edit -->
												<div class="sched-edit-block">
													<div class="sched-days-row">
														{#each ALL_DAYS as day}
															<button
																class="day-badge"
																class:day-badge-on={editSchedDays.includes(day)}
																onclick={() => toggleEditDay(day)}
																type="button"
															>{DAY_LABELS[day]}</button>
														{/each}
													</div>
													<div class="sched-edit-fields">
														<input type="time" bind:value={editSchedStart} class="time-input" />
														<span class="retention-unit">–</span>
														<input type="time" bind:value={editSchedEnd} class="time-input" />
														<select bind:value={editSchedFocusId} class="field-select">
															{#each sortedFocuses as f (f.id)}
																<option value={f.id}>{f.icon ? f.icon + ' ' : ''}{f.name}</option>
															{/each}
														</select>
														<button class="btn-add btn-sm" onclick={() => saveEditSchedule(sched)}>Save</button>
														<button class="btn-cancel btn-sm" onclick={cancelEditSchedule}>Cancel</button>
													</div>
												</div>
											{:else}
												<!-- Display row -->
												<div class="sched-display-row">
													<div class="sched-info">
														<div class="sched-days">
															{#each sched.days as day}
																<span class="day-badge day-badge-on">{DAY_LABELS[day]}</span>
															{/each}
														</div>
														<span class="sched-time">{formatHhmm(sched.time_range.start_hhmm)}–{formatHhmm(sched.time_range.end_hhmm)}</span>
														<span class="sched-arrow">→</span>
														<span class="sched-focus-name">{focusNameById(sched.focus_id)}</span>
													</div>
													<div class="sched-controls">
														<label class="toggle" title={sched.enabled ? 'Disable' : 'Enable'}>
															<input
																type="checkbox"
																checked={sched.enabled}
																onchange={(e) => handleToggleSchedule(sched, (e.target as HTMLInputElement).checked)}
															/>
															<span class="toggle-track"></span>
														</label>
														<button class="btn-action" onclick={() => startEditSchedule(sched)} title="Edit">✎</button>
														<button class="btn-remove" onclick={() => handleRemoveSchedule(sched.id)} title="Delete">✕</button>
													</div>
												</div>
											{/if}
										</li>
									{/each}
								</ul>
							{/if}

							{#if schedulesError}
								<p class="add-error">{schedulesError}</p>
							{/if}

							<!-- Add schedule form -->
							<div class="sched-add-block">
								<h4 class="add-title">Add schedule</h4>
								<div class="sched-days-row">
									{#each ALL_DAYS as day}
										<button
											class="day-badge"
											class:day-badge-on={newSchedDays.includes(day)}
											onclick={() => toggleNewDay(day)}
											type="button"
										>{DAY_LABELS[day]}</button>
									{/each}
								</div>
								<div class="sched-add-fields">
									<input type="time" bind:value={newSchedStart} class="time-input" />
									<span class="retention-unit">–</span>
									<input type="time" bind:value={newSchedEnd} class="time-input" />
									<select bind:value={newSchedFocusId} class="field-select">
										<option value="">Select focus…</option>
										{#each sortedFocuses as f (f.id)}
											<option value={f.id}>{f.icon ? f.icon + ' ' : ''}{f.name}</option>
										{/each}
									</select>
									<button
										class="btn-add btn-sm"
										onclick={handleAddSchedule}
										disabled={schedulesBusy || newSchedDays.length === 0 || !newSchedFocusId}
									>
										{schedulesBusy ? 'Adding…' : 'Add'}
									</button>
								</div>
							</div>
						</div>

						<!-- Add focus form -->
						<div class="add-form">
							<h3 class="add-title">Add focus</h3>
							<div class="add-fields">
								<input
									type="text"
									bind:value={newFocusIcon}
									placeholder="icon (emoji)"
									class="field-icon"
									maxlength="4"
								/>
								<input
									type="text"
									bind:value={newFocusName}
									placeholder="Focus name"
									class="field-focus-name"
								/>
								<button
									class="btn-add"
									onclick={handleAddFocus}
									disabled={focusesBusy || !newFocusName.trim()}
								>
									{focusesBusy ? 'Adding…' : 'Add'}
								</button>
							</div>
							{#if focusesError}
								<p class="add-error">{focusesError}</p>
							{/if}
						</div>

					</div>

				<!-- Import / Export sub-panel (3H) -->
				{:else if settingsTab === 'import-export'}
					<div class="filters-panel">

						<!-- Export section -->
						<div class="filters-section">
							<h3 class="section-title">Export config</h3>
							<p class="section-hint">Export your full configuration (producers, rules, focuses, schedules, retention) as JSON. Copy the text below to back it up or move it to another machine.</p>
							<div>
								<button class="btn-add btn-sm" onclick={handleExportConfig}>
									Export config
								</button>
							</div>
							{#if exportJson}
								<p class="export-char-count">{exportCharCount} characters</p>
								<textarea
									id="export-textarea"
									class="config-textarea"
									readonly
									value={exportJson}
								></textarea>
							{/if}
						</div>

						<!-- Import section -->
						<div class="filters-section">
							<h3 class="section-title">Import config</h3>
							<p class="section-hint">Paste a previously exported JSON config below. This will overwrite your current configuration and reconnect all producers.</p>
							<textarea
								class="config-textarea"
								placeholder="Paste config JSON here…"
								bind:value={importJson}
							></textarea>
							<div class="import-footer">
								<button
									class="btn-add btn-sm"
									onclick={handleImportConfig}
									disabled={importBusy || !importJson.trim()}
								>
									{importBusy ? 'Importing…' : 'Import config'}
								</button>
							</div>
							{#if importError}
								<p class="add-error">{importError}</p>
							{/if}
							{#if importSuccess}
								<p class="import-success">{importSuccess}</p>
							{/if}
						</div>

					</div>

				<!-- Theme sub-panel (3I) -->
				{:else if settingsTab === 'theme'}
					<div class="filters-panel">

						<div class="filters-section">
							<h3 class="section-title">Custom CSS</h3>
							<p class="section-hint">Paste custom CSS to override any app styles. Changes apply live.</p>
							<textarea
								class="config-textarea css-textarea"
								placeholder={"/* e.g. body { font-size: 14px; } */"}
								bind:value={customCssDraft}
								spellcheck="false"
							></textarea>
							<div class="theme-footer">
								<button
									class="btn-add btn-sm"
									onclick={handleApplyCss}
									disabled={customCssBusy}
								>
									{customCssBusy ? 'Applying…' : 'Apply'}
								</button>
								<button
									class="btn-cancel btn-sm"
									onclick={handleResetCss}
									disabled={customCssBusy}
								>
									Reset
								</button>
							</div>
							{#if customCssError}
								<p class="add-error">{customCssError}</p>
							{/if}
							{#if customCssSuccess}
								<p class="import-success">{customCssSuccess}</p>
							{/if}
						</div>

					</div>

				{/if}

			</section>
		{/if}

	</main>
</div>

<style>
	:global(body) {
		margin: 0;
		background: #0f1115;
		color: #e6e6e6;
		font-family: system-ui, -apple-system, sans-serif;
	}

	/* Shell layout */

	.app-shell {
		display: flex;
		height: 100vh;
		overflow: hidden;
	}

	/* Focuses sidebar */

	.focuses-sidebar {
		width: 130px;
		flex-shrink: 0;
		background: #0d1018;
		border-right: 1px solid #1c1f2e;
		display: flex;
		flex-direction: column;
		padding: 0;
		overflow-y: auto;
	}

	.focuses-brand {
		font-size: 0.82rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		padding: 0.9rem 0.75rem 0.6rem;
		border-bottom: 1px solid #1c1f2e;
		color: #e6e6e6;
	}

	.focuses-label {
		font-size: 0.65rem;
		font-weight: 600;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: #4b526e;
		padding: 0.55rem 0.75rem 0.2rem;
	}

	.focus-item {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		background: none;
		border: none;
		color: #7a849e;
		font-size: 0.82rem;
		text-align: left;
		padding: 0.38rem 0.75rem;
		cursor: pointer;
		width: 100%;
		transition: color 0.12s, background 0.12s;
		border-left: 2px solid transparent;
		line-height: 1.3;
	}

	.focus-item:hover {
		color: #c0c8e0;
		background: #131620;
	}

	.focus-item.active {
		color: #e6e6e6;
		background: #161a28;
		border-left-color: #3b82f6;
	}

	.focus-icon {
		font-size: 0.88rem;
		flex-shrink: 0;
		line-height: 1;
	}

	.focus-icon-empty {
		color: #3a3f55;
	}

	.focus-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.focuses-footer {
		margin-top: auto;
		padding: 0.55rem 0.75rem;
		border-top: 1px solid #1c1f2e;
	}

	.btn-add-focus {
		background: none;
		border: 1px solid #22263a;
		color: #5a6480;
		font-size: 0.75rem;
		padding: 0.3rem 0.6rem;
		border-radius: 4px;
		cursor: pointer;
		width: 100%;
		text-align: center;
		transition: color 0.12s, background 0.12s, border-color 0.12s;
	}

	.btn-add-focus:hover {
		color: #e6e6e6;
		background: #1a1d28;
		border-color: #3b82f6;
	}

	/* Nav sidebar */

	.sidebar {
		width: 120px;
		flex-shrink: 0;
		background: #13161d;
		border-right: 1px solid #22263a;
		display: flex;
		flex-direction: column;
		padding: 1rem 0;
		gap: 0.25rem;
	}

	.nav-item {
		background: none;
		border: none;
		color: #a0a8be;
		font-size: 0.88rem;
		text-align: left;
		padding: 0.5rem 1rem;
		cursor: pointer;
		border-radius: 0;
		transition: color 0.15s, background 0.15s;
	}

	.nav-item:hover {
		color: #e6e6e6;
		background: #1a1d28;
	}

	.nav-item.active {
		color: #e6e6e6;
		background: #1e2232;
		border-left: 2px solid #3b82f6;
	}

	/* Content area */

	.content {
		flex: 1;
		overflow-y: auto;
		padding: 1.25rem;
		box-sizing: border-box;
	}

	.panel {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.panel-title {
		font-size: 1.05rem;
		font-weight: 600;
		margin: 0 0 0.25rem;
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}

	.focus-badge {
		font-size: 0.72rem;
		font-weight: 500;
		padding: 0.12rem 0.5rem;
		border-radius: 10px;
		background: #1e2845;
		color: #93c5fd;
		border: 1px solid #2d4070;
	}

	/* Notifications list */

	.notif-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.notification-item {
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 6px;
		padding: 0.55rem 0.75rem;
	}

	.notif-header {
		display: flex;
		align-items: baseline;
		gap: 0.4rem;
	}

	.app-name {
		font-size: 0.75rem;
		color: #7dd3fc;
		opacity: 0.9;
		white-space: nowrap;
	}

	.notif-title {
		font-weight: 600;
		font-size: 0.9rem;
		flex: 1;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.timestamp {
		font-size: 0.72rem;
		opacity: 0.4;
		white-space: nowrap;
	}

	.notif-body {
		margin: 0.2rem 0 0 0;
		font-size: 0.82rem;
		opacity: 0.75;
		line-height: 1.4;
	}

	.notif-meta {
		margin: 0.15rem 0 0 0;
		font-size: 0.72rem;
		color: #6b7280;
		opacity: 0.8;
	}

	/* History toolbar */

	.history-toolbar {
		display: flex;
		gap: 0.6rem;
		align-items: center;
		flex-wrap: wrap;
	}

	.history-filter-input {
		flex: 1;
		min-width: 160px;
		padding: 0.42rem 0.65rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
	}

	.history-filter-input:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.btn-prune {
		padding: 0.42rem 0.85rem;
		background: #1e2232;
		color: #a0a8be;
		border: 1px solid #2e3240;
		border-radius: 5px;
		font-size: 0.85rem;
		cursor: pointer;
		white-space: nowrap;
		flex-shrink: 0;
		transition: color 0.15s, background 0.15s;
	}

	.btn-prune:hover:not(:disabled) {
		color: #e6e6e6;
		background: #2563eb;
		border-color: #2563eb;
	}

	.btn-prune:disabled {
		opacity: 0.45;
		cursor: default;
	}

	.prune-result {
		font-size: 0.82rem;
		color: #86efac;
		margin: 0;
	}

	.btn-load-more {
		padding: 0.42rem 1rem;
		background: #1e2232;
		color: #a0a8be;
		border: 1px solid #2e3240;
		border-radius: 5px;
		font-size: 0.85rem;
		cursor: pointer;
		transition: color 0.15s, background 0.15s;
		align-self: center;
	}

	.btn-load-more:hover:not(:disabled) {
		color: #e6e6e6;
		background: #22263a;
	}

	.btn-load-more:disabled {
		opacity: 0.45;
		cursor: default;
	}

	/* Producers list */

	.producer-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.producer-row {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 6px;
		padding: 0.55rem 0.75rem;
	}

	.status-dot {
		font-size: 0.9rem;
		line-height: 1;
		flex-shrink: 0;
	}

	.producer-info {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 0.1rem;
		overflow: hidden;
	}

	.producer-label {
		font-size: 0.9rem;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.producer-url {
		font-size: 0.72rem;
		color: #7dd3fc;
		opacity: 0.7;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.producer-error {
		font-size: 0.72rem;
		color: #f87171;
		opacity: 0.85;
	}

	/* Toggle switch */

	.toggle {
		position: relative;
		display: inline-flex;
		align-items: center;
		cursor: pointer;
		flex-shrink: 0;
	}

	.toggle input {
		position: absolute;
		opacity: 0;
		width: 0;
		height: 0;
	}

	.toggle-track {
		display: block;
		width: 32px;
		height: 18px;
		background: #2e3240;
		border-radius: 9px;
		transition: background 0.2s;
	}

	.toggle-track::after {
		content: '';
		display: block;
		width: 12px;
		height: 12px;
		background: #6b7280;
		border-radius: 50%;
		margin: 3px;
		transition: transform 0.2s, background 0.2s;
	}

	.toggle input:checked + .toggle-track {
		background: #1d4ed8;
	}

	.toggle input:checked + .toggle-track::after {
		transform: translateX(14px);
		background: #60a5fa;
	}

	/* Remove button */

	.btn-remove {
		background: none;
		border: none;
		color: #6b7280;
		font-size: 0.8rem;
		cursor: pointer;
		padding: 0.2rem 0.35rem;
		border-radius: 4px;
		flex-shrink: 0;
		line-height: 1;
		transition: color 0.15s, background 0.15s;
	}

	.btn-remove:hover {
		color: #f87171;
		background: #2e1f1f;
	}

	/* Add-producer form */

	.add-form {
		margin-top: 0.5rem;
		background: #13161d;
		border: 1px solid #22263a;
		border-radius: 8px;
		padding: 0.85rem 1rem;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}

	.add-title {
		font-size: 0.85rem;
		font-weight: 600;
		margin: 0;
		color: #a0a8be;
	}

	.add-fields {
		display: flex;
		gap: 0.5rem;
		flex-wrap: wrap;
	}

	.field-url {
		flex: 2;
		min-width: 160px;
	}

	.field-label {
		flex: 1;
		min-width: 110px;
	}

	.field-url,
	.field-label {
		padding: 0.42rem 0.65rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
	}

	.field-url:focus,
	.field-label:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.btn-add {
		padding: 0.42rem 1rem;
		background: #2563eb;
		color: #fff;
		border: none;
		border-radius: 5px;
		font-size: 0.88rem;
		cursor: pointer;
		white-space: nowrap;
		flex-shrink: 0;
		transition: background 0.15s;
	}

	.btn-add:hover:not(:disabled) {
		background: #3b82f6;
	}

	.btn-add:disabled {
		opacity: 0.45;
		cursor: default;
	}

	.btn-cancel {
		padding: 0.42rem 1rem;
		background: #1e2232;
		color: #a0a8be;
		border: 1px solid #2e3240;
		border-radius: 5px;
		font-size: 0.88rem;
		cursor: pointer;
		white-space: nowrap;
		flex-shrink: 0;
		transition: background 0.15s, color 0.15s;
	}

	.btn-cancel:hover {
		background: #22263a;
		color: #e6e6e6;
	}

	.btn-sm {
		padding: 0.3rem 0.7rem;
		font-size: 0.82rem;
	}

	.add-error {
		color: #f87171;
		font-size: 0.8rem;
		margin: 0;
	}

	/* Settings tabs */

	.settings-tabs {
		display: flex;
		gap: 0;
		border-bottom: 1px solid #22263a;
		margin-bottom: 0.75rem;
	}

	.settings-tab {
		background: none;
		border: none;
		border-bottom: 2px solid transparent;
		color: #a0a8be;
		font-size: 0.88rem;
		padding: 0.4rem 1rem;
		cursor: pointer;
		margin-bottom: -1px;
		transition: color 0.15s, border-color 0.15s;
	}

	.settings-tab:hover {
		color: #e6e6e6;
	}

	.settings-tab.active {
		color: #e6e6e6;
		border-bottom-color: #3b82f6;
	}

	/* Filters panel */

	.filters-panel {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}

	.filters-section {
		background: #13161d;
		border: 1px solid #22263a;
		border-radius: 8px;
		padding: 0.85rem 1rem;
		display: flex;
		flex-direction: column;
		gap: 0.6rem;
	}

	.section-title {
		font-size: 0.85rem;
		font-weight: 600;
		margin: 0;
		color: #a0a8be;
	}

	.section-hint {
		font-size: 0.78rem;
		color: #5a6480;
		margin: 0;
		line-height: 1.4;
	}

	.section-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	/* Default mode radio row */

	.mode-row {
		display: flex;
		gap: 1.5rem;
	}

	.mode-option {
		display: flex;
		align-items: center;
		gap: 0.45rem;
		cursor: pointer;
		font-size: 0.88rem;
	}

	.mode-option input[type='radio'],
	.mode-option input[type='checkbox'] {
		accent-color: #3b82f6;
	}

	/* Refresh button */

	.btn-refresh {
		background: none;
		border: 1px solid #2e3240;
		color: #a0a8be;
		font-size: 0.78rem;
		padding: 0.2rem 0.6rem;
		border-radius: 4px;
		cursor: pointer;
		transition: color 0.15s, background 0.15s;
	}

	.btn-refresh:hover {
		color: #e6e6e6;
		background: #1e2232;
	}

	/* App list */

	.app-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}

	.app-row {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.35rem 0;
	}

	.app-row-name {
		flex: 1;
		font-size: 0.88rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.app-rule-buttons {
		display: flex;
		gap: 0;
		flex-shrink: 0;
	}

	.rule-btn {
		background: #1a1d24;
		border: 1px solid #2e3240;
		color: #6b7280;
		font-size: 0.75rem;
		padding: 0.22rem 0.6rem;
		cursor: pointer;
		transition: color 0.12s, background 0.12s;
	}

	.rule-btn:first-child {
		border-radius: 4px 0 0 4px;
	}

	.rule-btn:last-child {
		border-radius: 0 4px 4px 0;
	}

	.rule-btn:not(:first-child) {
		border-left: none;
	}

	.rule-btn:hover {
		color: #e6e6e6;
		background: #22263a;
	}

	.rule-btn.active-allow {
		background: #14532d;
		color: #86efac;
		border-color: #166534;
	}

	.rule-btn.active-default {
		background: #1e2232;
		color: #e6e6e6;
		border-color: #3b82f6;
	}

	.rule-btn.active-block {
		background: #450a0a;
		color: #fca5a5;
		border-color: #7f1d1d;
	}

	/* Keyword filter list */

	.kw-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}

	.kw-row {
		display: flex;
		align-items: center;
		gap: 0.45rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		padding: 0.35rem 0.6rem;
		flex-wrap: wrap;
	}

	.badge {
		font-size: 0.7rem;
		padding: 0.12rem 0.45rem;
		border-radius: 3px;
		font-weight: 600;
		white-space: nowrap;
	}

	.field-badge {
		background: #1e2845;
		color: #93c5fd;
	}

	.badge-allow {
		background: #14532d;
		color: #86efac;
	}

	.badge-block {
		background: #450a0a;
		color: #fca5a5;
	}

	.badge-active {
		background: #1a3a1a;
		color: #86efac;
		font-size: 0.68rem;
		padding: 0.1rem 0.4rem;
	}

	.kw-contains {
		font-size: 0.78rem;
		color: #6b7280;
		white-space: nowrap;
	}

	.kw-keyword {
		font-size: 0.82rem;
		font-family: monospace;
		color: #fde68a;
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	/* Add keyword filter form */

	.kw-add-form {
		margin-top: 0.2rem;
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
	}

	.kw-add-fields {
		display: flex;
		gap: 0.45rem;
		flex-wrap: wrap;
	}

	.field-select {
		padding: 0.42rem 0.55rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
	}

	.field-select:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.field-keyword {
		flex: 1;
		min-width: 120px;
		padding: 0.42rem 0.65rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
	}

	.field-keyword:focus {
		outline: none;
		border-color: #3b82f6;
	}

	/* Retention panel */

	.retention-row {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		flex-wrap: wrap;
	}

	.retention-label {
		font-size: 0.88rem;
		color: #a0a8be;
		white-space: nowrap;
	}

	.retention-days-input {
		width: 70px;
		padding: 0.38rem 0.55rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
		text-align: right;
	}

	.retention-days-input:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.retention-days-input:disabled {
		opacity: 0.4;
	}

	.retention-days-input-sm {
		width: 60px;
		padding: 0.3rem 0.45rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.85rem;
		text-align: right;
	}

	.retention-days-input-sm:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.retention-unit {
		font-size: 0.82rem;
		color: #6b7280;
		white-space: nowrap;
	}

	.retention-producer-row {
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		padding: 0.35rem 0.6rem;
	}

	/* Focuses settings panel */

	.focus-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.focus-list-item {
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 6px;
		overflow: hidden;
	}

	.focus-display-row {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.5rem 0.75rem;
	}

	.focus-edit-row {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		padding: 0.5rem 0.75rem;
		flex-wrap: wrap;
	}

	.focus-list-icon {
		font-size: 1rem;
		flex-shrink: 0;
		line-height: 1;
	}

	.focus-list-name {
		flex: 1;
		font-size: 0.9rem;
		font-weight: 500;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.focus-actions {
		display: flex;
		align-items: center;
		gap: 0.2rem;
		flex-shrink: 0;
	}

	.btn-action {
		background: none;
		border: 1px solid #2e3240;
		color: #6b7280;
		font-size: 0.75rem;
		cursor: pointer;
		padding: 0.2rem 0.45rem;
		border-radius: 4px;
		line-height: 1;
		transition: color 0.12s, background 0.12s;
		white-space: nowrap;
	}

	.btn-action:hover {
		color: #e6e6e6;
		background: #1e2232;
		border-color: #3b82f6;
	}

	.field-icon {
		width: 52px;
		padding: 0.42rem 0.4rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 1.1rem;
		text-align: center;
		flex-shrink: 0;
	}

	.field-icon:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.field-focus-name {
		flex: 1;
		min-width: 120px;
		padding: 0.42rem 0.65rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
	}

	.field-focus-name:focus {
		outline: none;
		border-color: #3b82f6;
	}

	/* Focus inline rules editor */

	.focus-rules-editor {
		border-top: 1px solid #22263a;
		background: #0f1117;
		padding: 0.75rem 1rem;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}

	.focus-rules-section {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.focus-rules-label {
		font-size: 0.78rem;
		font-weight: 600;
		color: #6b7280;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.focus-rules-footer {
		display: flex;
		gap: 0.5rem;
		padding-top: 0.25rem;
		border-top: 1px solid #1c1f2e;
	}

	/* Misc */

	.empty {
		opacity: 0.35;
		font-size: 0.85rem;
		margin: 0.25rem 0;
	}

	/* Schedules */

	.schedule-list {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}

	.schedule-item {
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 6px;
		overflow: hidden;
	}

	.sched-disabled {
		opacity: 0.5;
	}

	.sched-display-row {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.5rem 0.75rem;
		flex-wrap: wrap;
	}

	.sched-info {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 0.4rem;
		flex-wrap: wrap;
		min-width: 0;
	}

	.sched-days {
		display: flex;
		gap: 0.2rem;
		flex-wrap: wrap;
	}

	.sched-time {
		font-size: 0.85rem;
		font-family: monospace;
		color: #fde68a;
		white-space: nowrap;
	}

	.sched-arrow {
		font-size: 0.8rem;
		color: #4b526e;
	}

	.sched-focus-name {
		font-size: 0.85rem;
		color: #93c5fd;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.sched-controls {
		display: flex;
		align-items: center;
		gap: 0.3rem;
		flex-shrink: 0;
	}

	.sched-edit-block,
	.sched-add-block {
		padding: 0.6rem 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.45rem;
	}

	.sched-edit-block {
		border-top: 1px solid #22263a;
		background: #0f1117;
	}

	.sched-add-block {
		border-top: 1px solid #22263a;
		margin-top: 0.2rem;
		padding-top: 0.6rem;
	}

	.sched-days-row {
		display: flex;
		gap: 0.25rem;
		flex-wrap: wrap;
	}

	.sched-edit-fields,
	.sched-add-fields {
		display: flex;
		gap: 0.4rem;
		align-items: center;
		flex-wrap: wrap;
	}

	.day-badge {
		font-size: 0.72rem;
		font-weight: 600;
		padding: 0.15rem 0.45rem;
		border-radius: 3px;
		background: #1a1d24;
		border: 1px solid #2e3240;
		color: #4b526e;
		cursor: pointer;
		transition: background 0.12s, color 0.12s, border-color 0.12s;
		line-height: 1.4;
		white-space: nowrap;
	}

	.day-badge:hover {
		color: #c0c8e0;
		background: #22263a;
	}

	.day-badge-on {
		background: #1e2845;
		color: #93c5fd;
		border-color: #2d4070;
	}

	.time-input {
		padding: 0.38rem 0.5rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.88rem;
		width: 108px;
		flex-shrink: 0;
	}

	.time-input:focus {
		outline: none;
		border-color: #3b82f6;
	}

	/* Import / Export + Theme panels */

	.config-textarea {
		width: 100%;
		min-height: 200px;
		padding: 0.6rem 0.75rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.82rem;
		font-family: monospace;
		resize: vertical;
		box-sizing: border-box;
		line-height: 1.5;
	}

	.config-textarea:focus {
		outline: none;
		border-color: #3b82f6;
	}

	.css-textarea {
		min-height: 260px;
	}

	.export-char-count {
		font-size: 0.75rem;
		color: #6b7280;
		margin: 0;
	}

	.import-footer,
	.theme-footer {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}

	.import-success {
		color: #86efac;
		font-size: 0.82rem;
		margin: 0;
	}
</style>
