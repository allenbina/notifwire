<script lang="ts">
	// D0 placeholder shell. The real UI — focuses, the menubar picker, filters,
	// settings — lands in D3. This just proves the Tauri + SvelteKit window boots.
	const version = '0.0.0';
</script>

<main>
	<h1>notifwire</h1>
	<p class="tagline">a self-hosted, peer-to-peer notification mesh</p>
	<p class="status">app shell · v{version} · D0</p>
</main>

<style>
	:global(body) {
		margin: 0;
		background: #0f1115;
		color: #e6e6e6;
	}
	main {
		font-family: system-ui, -apple-system, sans-serif;
		min-height: 100vh;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 0.25rem;
	}
	h1 {
		font-weight: 700;
		letter-spacing: -0.02em;
		margin: 0;
	}
	.tagline {
		opacity: 0.8;
		margin: 0;
	}
	.status {
		opacity: 0.45;
		font-size: 0.85rem;
		margin-top: 0.5rem;
	}
</style>
