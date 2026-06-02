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

	interface ProducerStatus {
		url: string;
		state: 'connecting' | 'connected' | 'reconnecting' | 'unreachable';
		last_event_unix_ms: number | null;
		last_error: string | null;
	}

	interface ConsumerHealth {
		status: 'ok' | 'degraded' | 'unhealthy';
		self_checks: { history_ok: boolean; pipeline_alive: boolean; detail: string | null };
		producers: ProducerStatus[];
	}

	// ---------------------------------------------------------------------------
	// State (Svelte 5 runes)
	// ---------------------------------------------------------------------------

	let producerUrl = $state('http://localhost:8765');
	let connected = $state(false);
	let connecting = $state(false);
	let errorMsg = $state('');
	let health = $state<ConsumerHealth | null>(null);
	let notifications = $state<NotificationItem[]>([]);

	// ---------------------------------------------------------------------------
	// Lifecycle
	// ---------------------------------------------------------------------------

	let unlisten: UnlistenFn | null = null;
	let healthInterval: ReturnType<typeof setInterval> | null = null;

	onMount(async () => {
		unlisten = await listen<NotificationItem>('notification', (event) => {
			// Prepend newest-first; cap at 100.
			notifications = [event.payload, ...notifications].slice(0, 100);
		});
	});

	onDestroy(() => {
		unlisten?.();
		if (healthInterval) clearInterval(healthInterval);
	});

	// ---------------------------------------------------------------------------
	// Connect / Disconnect
	// ---------------------------------------------------------------------------

	async function handleConnect() {
		if (!producerUrl.trim()) return;
		errorMsg = '';
		connecting = true;
		try {
			await invoke('connect', { producerUrl: producerUrl.trim() });
			connected = true;
			startHealthPolling();
		} catch (e) {
			errorMsg = String(e);
		} finally {
			connecting = false;
		}
	}

	async function handleDisconnect() {
		errorMsg = '';
		try {
			await invoke('disconnect');
		} catch (e) {
			errorMsg = String(e);
		}
		connected = false;
		health = null;
		stopHealthPolling();
	}

	// ---------------------------------------------------------------------------
	// Health polling
	// ---------------------------------------------------------------------------

	function startHealthPolling() {
		pollHealth();
		healthInterval = setInterval(pollHealth, 3000);
	}

	function stopHealthPolling() {
		if (healthInterval) {
			clearInterval(healthInterval);
			healthInterval = null;
		}
	}

	async function pollHealth() {
		try {
			health = await invoke<ConsumerHealth>('get_health');
		} catch {
			// Silently ignore — connection may have just dropped.
		}
	}

	// ---------------------------------------------------------------------------
	// Status dot helpers
	// ---------------------------------------------------------------------------

	function statusDot(h: ConsumerHealth | null): string {
		if (!h) return '⚪';
		const state = h.producers[0]?.state;
		if (!state) return '⚪';
		switch (state) {
			case 'connected': return '🟢';
			case 'reconnecting': return '🟡';
			case 'connecting': return '🟡';
			case 'unreachable': return '🔴';
		}
	}

	function statusLabel(h: ConsumerHealth | null): string {
		if (!h) return 'Disconnected';
		const state = h.producers[0]?.state;
		if (!state) return 'Disconnected';
		switch (state) {
			case 'connected': return 'Connected';
			case 'reconnecting': return 'Reconnecting';
			case 'connecting': return 'Connecting';
			case 'unreachable': return 'Unreachable';
		}
	}

	function relativeTime(ms: number): string {
		const diff = Math.floor((Date.now() - ms) / 1000);
		if (diff < 5) return 'just now';
		if (diff < 60) return `${diff}s ago`;
		if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
		return `${Math.floor(diff / 3600)}h ago`;
	}
</script>

<main>
	<header>
		<h1>notifwire</h1>
	</header>

	<section class="connect-section">
		<input
			type="text"
			bind:value={producerUrl}
			placeholder="http://localhost:8765"
			disabled={connected || connecting}
		/>
		{#if !connected}
			<button onclick={handleConnect} disabled={connecting || !producerUrl.trim()}>
				{connecting ? 'Connecting…' : 'Connect'}
			</button>
		{:else}
			<button class="disconnect" onclick={handleDisconnect}>Disconnect</button>
		{/if}
	</section>

	{#if errorMsg}
		<p class="error">{errorMsg}</p>
	{/if}

	{#if connected || health}
		<div class="status-row">
			<span class="dot">{statusDot(health)}</span>
			<span class="label">{statusLabel(health)}</span>
			{#if health?.producers[0]?.last_error}
				<span class="error-detail">{health.producers[0].last_error}</span>
			{/if}
		</div>
	{/if}

	<section class="notifications">
		{#if notifications.length === 0}
			<p class="empty">No notifications yet.</p>
		{:else}
			<ul>
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
</main>

<style>
	:global(body) {
		margin: 0;
		background: #0f1115;
		color: #e6e6e6;
		font-family: system-ui, -apple-system, sans-serif;
	}

	main {
		display: flex;
		flex-direction: column;
		min-height: 100vh;
		padding: 1.25rem;
		box-sizing: border-box;
		gap: 0.75rem;
	}

	header h1 {
		font-size: 1.3rem;
		font-weight: 700;
		letter-spacing: -0.02em;
		margin: 0 0 0.25rem 0;
	}

	/* Connect section */
	.connect-section {
		display: flex;
		gap: 0.5rem;
	}

	.connect-section input {
		flex: 1;
		padding: 0.45rem 0.7rem;
		background: #1a1d24;
		border: 1px solid #2e3240;
		border-radius: 5px;
		color: #e6e6e6;
		font-size: 0.9rem;
	}

	.connect-section input:disabled {
		opacity: 0.5;
	}

	.connect-section button {
		padding: 0.45rem 1rem;
		background: #3b82f6;
		color: #fff;
		border: none;
		border-radius: 5px;
		font-size: 0.9rem;
		cursor: pointer;
		white-space: nowrap;
	}

	.connect-section button:disabled {
		opacity: 0.45;
		cursor: default;
	}

	.connect-section button.disconnect {
		background: #dc2626;
	}

	/* Error message */
	.error {
		color: #f87171;
		font-size: 0.82rem;
		margin: 0;
	}

	/* Status row */
	.status-row {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.88rem;
	}

	.dot {
		font-size: 1rem;
		line-height: 1;
	}

	.label {
		opacity: 0.85;
	}

	.error-detail {
		color: #f87171;
		font-size: 0.8rem;
		opacity: 0.8;
		margin-left: 0.4rem;
	}

	/* Notifications list */
	.notifications {
		flex: 1;
		overflow-y: auto;
	}

	.empty {
		opacity: 0.35;
		font-size: 0.85rem;
		margin: 1rem 0 0 0;
	}

	ul {
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
</style>
