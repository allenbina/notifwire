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

	// ---------------------------------------------------------------------------
	// State (Svelte 5 runes)
	// ---------------------------------------------------------------------------

	type Panel = 'notifications' | 'settings';
	let activePanel = $state<Panel>('notifications');

	// Notifications panel
	let notifications = $state<NotificationItem[]>([]);

	// Settings panel — producers list
	let producers = $state<ProducerEntry[]>([]);
	let statuses = $state<ProducerStatus[]>([]);

	// Add-producer form
	let newUrl = $state('');
	let newLabel = $state('');
	let addError = $state('');
	let addBusy = $state(false);

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

	// ---------------------------------------------------------------------------
	// Settings actions
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
			class:active={activePanel === 'settings'}
			onclick={() => { activePanel = 'settings'; }}
		>
			Settings
		</button>
	</nav>

	<!-- Right content -->
	<main class="content">

		<!-- ── Notifications panel ── -->
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

		<!-- ── Settings panel ── -->
		{:else}
			<section class="panel">
				<h2 class="panel-title">Producers</h2>

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

	/* ── Shell layout ── */

	.app-shell {
		display: flex;
		height: 100vh;
		overflow: hidden;
	}

	/* ── Sidebar ── */

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

	/* ── Content area ── */

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

	/* ── Notifications list ── */

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

	/* ── Producers list ── */

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

	/* ── Add-producer form ── */

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

	.add-error {
		color: #f87171;
		font-size: 0.8rem;
		margin: 0;
	}

	/* ── Misc ── */

	.empty {
		opacity: 0.35;
		font-size: 0.85rem;
		margin: 0.25rem 0;
	}
</style>
