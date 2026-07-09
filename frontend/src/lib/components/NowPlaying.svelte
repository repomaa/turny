<script lang="ts">
	import type { NowPlaying } from '$lib/types';

	let {
		nowPlaying,
		progressPct
	}: {
		nowPlaying: NowPlaying;
		progressPct: number;
	} = $props();

	function fmt(ms: number): string {
		const clamped = Math.max(0, ms);
		const s = Math.floor(clamped / 1000);
		const m = Math.floor(s / 60);
		const r = s % 60;
		return `${m}:${r.toString().padStart(2, '0')}`;
	}
</script>

<div class="flex gap-4">
	{#if nowPlaying.album_art}
		<img
			src={nowPlaying.album_art}
			alt={`${nowPlaying.album} — ${nowPlaying.artist}`}
			class="h-24 w-24 rounded-lg object-cover"
		/>
	{:else}
		<div class="h-24 w-24 rounded-lg bg-gray-700"></div>
	{/if}
	<div class="flex-1 min-w-0">
		<p class="truncate text-xl font-semibold text-white">{nowPlaying.track_name}</p>
		<p class="truncate text-gray-400">{nowPlaying.artist}</p>
		<p class="truncate text-sm text-gray-500">{nowPlaying.album}</p>
		<div class="mt-3">
			<div class="h-1.5 w-full rounded-full bg-gray-700">
				<div
					class="h-1.5 rounded-full bg-green-500 transition-all"
					style="width: {progressPct}%"
				></div>
			</div>
			<div class="mt-1 flex justify-between text-xs text-gray-500">
				<span>{fmt(nowPlaying.progress_ms)}</span>
				<span>{fmt(nowPlaying.duration_ms)}</span>
			</div>
		</div>
	</div>
</div>
