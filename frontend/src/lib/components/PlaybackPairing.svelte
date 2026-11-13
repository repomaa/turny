<script lang="ts">
	import { onMount } from 'svelte';
	import { getPlaybackStatus, startPlaybackPairing, unpairPlayback } from '$lib/api';
	import type { PlaybackStatus } from '$lib/types';

	let { onerror }: { onerror?: (msg: string) => void } = $props();

	let status: PlaybackStatus | null = $state(null);
	let starting = $state(false);
	let unpairing = $state(false);

	async function refresh() {
		try {
			status = await getPlaybackStatus();
		} catch {
			// Server unreachable — keep the last known status
		}
	}

	async function pair() {
		starting = true;
		try {
			status = await startPlaybackPairing();
		} catch {
			onerror?.('Failed to start playback pairing');
		} finally {
			starting = false;
		}
	}

	async function unpair() {
		unpairing = true;
		try {
			await unpairPlayback();
			await refresh();
		} catch {
			onerror?.('Failed to unpair playback');
		} finally {
			unpairing = false;
		}
	}

	onMount(() => {
		refresh();
	});

	// Poll while waiting for the user to approve the code
	$effect(() => {
		if (status?.phase !== 'pending') return;
		const id = setInterval(refresh, 3000);
		return () => clearInterval(id);
	});
</script>

<section class="mb-6 rounded-lg bg-gray-800 p-6">
	<h2 class="mb-4 text-lg font-semibold text-gray-300">Playback Pairing</h2>

	{#if !status}
		<p class="text-gray-500">Checking playback status…</p>
	{:else if status.paired && status.phase !== 'pending'}
		<div class="flex items-center justify-between gap-4">
			<span
				class="inline-flex items-center gap-2 rounded-lg bg-green-500/20 px-3 py-1 text-sm font-medium text-green-400"
			>
				<svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" role="img">
					<title>Paired</title>
					<circle cx="12" cy="12" r="6" />
				</svg>
				Playback paired
			</span>
			<button
				class="rounded-lg bg-gray-700 px-4 py-2 text-sm font-semibold text-white hover:bg-gray-600 disabled:opacity-50"
				onclick={unpair}
				disabled={unpairing}
			>
				{unpairing ? 'Unpairing…' : 'Unpair'}
			</button>
		</div>
	{:else if status.phase === 'pending' && status.user_code}
		<div class="space-y-3 text-center">
			<p class="text-gray-400">
				On any device, open
				<a
					class="text-green-400 underline"
					href={status.verification_uri ?? 'https://spotify.com/pair'}
					target="_blank"
					rel="noreferrer">spotify.com/pair</a
				>
				and enter this code:
			</p>
			<p
				class="select-all rounded-lg bg-gray-900 px-4 py-3 font-mono text-3xl tracking-[0.35em] text-green-400"
			>
				{status.user_code}
			</p>
			<p class="flex items-center justify-center gap-2 text-sm text-gray-500">
				<svg
					class="h-4 w-4 animate-spin"
					viewBox="0 0 24 24"
					fill="none"
					stroke="currentColor"
					stroke-width="2"
					role="img"
				>
					<title>Waiting</title>
					<path d="M12 3a9 9 0 1 0 9 9" stroke-linecap="round" />
				</svg>
				Waiting for approval…
			</p>
		</div>
	{:else if status.phase === 'failed'}
		<div class="space-y-3 text-center">
			<p class="text-red-400">{status.error ?? 'Pairing failed.'}</p>
			<button
				class="rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600 disabled:opacity-50"
				onclick={pair}
				disabled={starting}
			>
				{starting ? 'Starting…' : 'Try again'}
			</button>
		</div>
	{:else}
		<div class="flex items-center justify-between gap-4">
			<p class="text-gray-400">Pair Turny with Spotify to enable playback on this device.</p>
			<button
				class="shrink-0 rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600 disabled:opacity-50"
				onclick={pair}
				disabled={starting}
			>
				{starting ? 'Starting…' : 'Pair playback'}
			</button>
		</div>
	{/if}
</section>
