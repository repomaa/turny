<script lang="ts">
	let {
		message,
		ondismiss
	}: {
		message: string | null;
		ondismiss: () => void;
	} = $props();
</script>

{#if message}
	<div class="mb-4 rounded-lg bg-red-900/50 border border-red-700 px-4 py-3 text-sm text-red-300">
		{message}
		<button class="ml-2 underline" onclick={ondismiss} aria-label="Dismiss error">dismiss</button>
	</div>
{/if}
