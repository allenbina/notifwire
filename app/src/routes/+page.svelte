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

	// ---------------------------------------------------------------------------
	// State (Svelte 5 runes)
	// ---------------------------------------------------------------------------

	type Panel = 'notifications' | 'history' | 'settings';
	type SettingsTab = 'producers' | 'filters' | 'retention';

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

	// ---------------------------------------------------------------------------
	// Lifecycle
	// ---------------------------------------------------------------------------

	let unlisten: UnlistenFn | null = null;
	let healthInterval: ReturnType<typeof setInterval> | null = null;

	onMount(async () => {
		// Listen for incoming notifications
		unlisten = await listen<NotificationItem>('notification', (event) => {
			notifications = [event.payload, ...notifications].slice(0, 100);
		});

		// Load initial producer list
		await refreshProducers();

		// Poll health every 3 s
		healthInterval = setInterval(async () => {
			await pollHealth();
		}, 3000);
		await pollHealth();
	});

	onDestroy(() => {
		unlisten?.();
		if (healthInterval) clearInterval(healthInterval);
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

	// Called when settings tab is opened so data is fresh
	async function onSettingsTabActivated() {
		if (settingsTab === 'filters') {
			await refreshFilters();
			await refreshSeenApps();
		} else if (settingsTab === 'retention') {
			await refreshRetention();
		}
	}

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
	<!-- Left nav -->
	<nav class="sidebar">
		<div class="brand">notifwire</div>
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
				<h2 class="panel-title">Notifications</h2>
				{#if notifications.length === 0}
					<p class="empty">No notifications yet.</p>
				{:else}
					<ul class="notif-list">
						{#each notifications as n (n.timestamp_ms + n.title)}
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
				<h2 class="panel-title">History</h2>

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
				{:else if historyItems.length === 0}
					<p class="empty">No notifications in history.</p>
				{:else}
					<ul class="notif-list">
						{#each historyItems as n (n.id)}
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
				{:else}
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

	/* Sidebar */

	.sidebar {
		width: 140px;
		flex-shrink: 0;
		background: #13161d;
		border-right: 1px solid #22263a;
		display: flex;
		flex-direction: column;
		padding: 1rem 0;
		gap: 0.25rem;
	}

	.brand {
		font-size: 1rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		padding: 0 1rem 0.75rem;
		border-bottom: 1px solid #22263a;
		margin-bottom: 0.5rem;
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

	/* Misc */

	.empty {
		opacity: 0.35;
		font-size: 0.85rem;
		margin: 0.25rem 0;
	}
</style>
