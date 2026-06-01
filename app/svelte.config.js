import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),
	kit: {
		// Tauri serves a static bundle, so prerender to plain files with an
		// SPA fallback (see src/routes/+layout.ts: prerender + ssr disabled).
		adapter: adapter({ fallback: '200.html' })
	}
};

export default config;
