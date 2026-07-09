<script lang="ts">
	let {
		playerBusy,
		isPlaying,
		onplay,
		onpause,
		onnext,
		onprevious
	}: {
		playerBusy: boolean;
		isPlaying: boolean;
		onplay: () => void;
		onpause: () => void;
		onnext: () => void;
		onprevious: () => void;
	} = $props();
</script>

<div class="mt-5 flex items-center justify-center gap-4">
	<button
		class="rounded-full bg-gray-700 p-3 text-white hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
		onclick={onprevious}
		disabled={playerBusy}
		aria-label="Previous"
	>
		<svg class="h-6 w-6" viewBox="0 0 24 24" fill="currentColor" role="img">
			<title>Previous</title>
			<path d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/>
		</svg>
	</button>

	{#if isPlaying}
		<button
			class="rounded-full bg-green-500 p-4 text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
			onclick={onpause}
			disabled={playerBusy}
			aria-label="Pause"
		>
			<svg class="h-7 w-7" viewBox="0 0 24 24" fill="currentColor" role="img">
				<title>Pause</title>
				<path d="M6 5h4v14H6zm8 0h4v14h-4z"/>
			</svg>
		</button>
	{:else}
		<button
			class="rounded-full bg-green-500 p-4 text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
			onclick={onplay}
			disabled={playerBusy}
			aria-label="Play"
		>
			<svg class="h-7 w-7" viewBox="0 0 24 24" fill="currentColor" role="img">
				<title>Play</title>
				<path d="M8 5v14l11-7z"/>
			</svg>
		</button>
	{/if}

	<button
		class="rounded-full bg-gray-700 p-3 text-white hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
		onclick={onnext}
		disabled={playerBusy}
		aria-label="Next"
	>
		<svg class="h-6 w-6" viewBox="0 0 24 24" fill="currentColor" role="img">
			<title>Next</title>
			<path d="M16 6h2v12h-2zM6 18l8.5-6L6 6z"/>
		</svg>
	</button>
</div>
