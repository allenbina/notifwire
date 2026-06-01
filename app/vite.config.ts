import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Tauri integration: fixed dev port, don't clear the screen (so Rust build
// errors stay visible), and ignore the src-tauri dir in the file watcher.
export default defineConfig({
	plugins: [sveltekit()],
	clearScreen: false,
	server: {
		port: 5173,
		strictPort: true,
		watch: {
			ignored: ['**/src-tauri/**']
		}
	}
});
