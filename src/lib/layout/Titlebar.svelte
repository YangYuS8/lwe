<script lang="ts">
  import { browser } from '$app/environment';

  const withCurrentWindow = async (action: 'minimize' | 'toggleMaximize' | 'close') => {
    if (!browser) {
      return;
    }

    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const appWindow = getCurrentWindow();

    if (action === 'minimize') {
      await appWindow.minimize();
    } else if (action === 'toggleMaximize') {
      await appWindow.toggleMaximize();
    } else {
      await appWindow.close();
    }
  };
</script>

<div class="lwe-titlebar" data-tauri-drag-region>
  <div class="flex items-center gap-3" data-tauri-drag-region>
    <span class="h-2.5 w-2.5 rounded-full bg-primary/70" aria-hidden="true"></span>
    <span class="text-[0.68rem] font-semibold uppercase tracking-[0.24em] text-muted-foreground" data-tauri-drag-region>
      LWE
    </span>
  </div>

  <div class="flex items-center gap-1.5">
    <button class="lwe-titlebar-button" type="button" aria-label="Minimize" onclick={() => void withCurrentWindow('minimize')}>−</button>
    <button class="lwe-titlebar-button" type="button" aria-label="Maximize or restore" onclick={() => void withCurrentWindow('toggleMaximize')}>□</button>
    <button class="lwe-titlebar-button hover:border-destructive/50 hover:bg-destructive/10 hover:text-destructive" type="button" aria-label="Close" onclick={() => void withCurrentWindow('close')}>×</button>
  </div>
</div>
