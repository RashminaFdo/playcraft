// ============================================================================
// PlayCraft — Native Windows Game Launcher Frontend Engine
// Author & Rights Holder: Rashmina Fernando (www.rashmina.dev)
// Native Tauri v2 IPC Bridge (window.__TAURI__.core.invoke)
// ============================================================================

(function () {
  'use strict';

  const hasTauri = () =>
    Boolean(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke);

  const tauriInvoke = async (cmd, args = {}) => {
    if (hasTauri()) {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    throw new Error('Tauri IPC unavailable');
  };

  // --------------------------------------------------------------------------
  // 1. Modern Web Guidance: Top-Layer Discrete Display & Light-Dismiss Helpers
  // --------------------------------------------------------------------------
  let supportsDisplayTransition;
  function canTransitionDisplay() {
    if (supportsDisplayTransition !== undefined) return supportsDisplayTransition;
    if (!window.CSS?.supports?.('transition-behavior', 'allow-discrete') || !document.body) {
      return false;
    }
    const probe = document.createElement('div');
    probe.style.cssText = 'transition: display 1s allow-discrete !important; display: block;';
    document.body.appendChild(probe);
    getComputedStyle(probe).display;
    probe.style.display = 'none';
    supportsDisplayTransition = getComputedStyle(probe).display === 'block';
    probe.remove();
    return supportsDisplayTransition;
  }

  function supportsTopLayerExit() {
    return window.CSS?.supports?.('overlay', 'auto') && canTransitionDisplay();
  }

  async function closeTopLayer(dialog) {
    if (!dialog || !dialog.open) return;
    if (!supportsTopLayerExit()) {
      dialog.setAttribute('data-closing', '');
      const animations = dialog.getAnimations({ subtree: true });
      if (animations.length > 0) {
        await Promise.race([
          Promise.allSettled(animations.map((a) => a.finished)),
          new Promise((r) => setTimeout(r, 2000)),
        ]);
      }
      if (!dialog.hasAttribute('data-closing')) return;
      dialog.removeAttribute('data-closing');
    }
    dialog.close();
  }

  function setupModernDialogs() {
    document.querySelectorAll('dialog.ps5-dialog').forEach((dialog) => {
      dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        closeTopLayer(dialog);
      });

      if (!('closedBy' in HTMLDialogElement.prototype)) {
        dialog.addEventListener('click', (event) => {
          if (event.target !== dialog) return;
          const rect = dialog.getBoundingClientRect();
          const isDialogContent =
            rect.top <= event.clientY &&
            event.clientY <= rect.top + rect.height &&
            rect.left <= event.clientX &&
            event.clientX <= rect.left + rect.width;
          if (isDialogContent) return;
          closeTopLayer(dialog);
        });
      }
    });

    document.querySelectorAll('[data-close-dialog]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const id = btn.getAttribute('data-close-dialog');
        const dlg = document.getElementById(id);
        if (dlg) closeTopLayer(dlg);
      });
    });
  }

  // --------------------------------------------------------------------------
  // 2. WebAudio Synthesized Navigation Sound Engine
  // --------------------------------------------------------------------------
  class Ps5SoundEngine {
    constructor() {
      this.ctx = null;
      this.enabled = true;
    }

    ensureContext() {
      if (!this.enabled) return null;
      if (!this.ctx) {
        const AudioCtx = window.AudioContext || window.webkitAudioContext;
        if (AudioCtx) this.ctx = new AudioCtx();
      }
      if (this.ctx && this.ctx.state === 'suspended') {
        this.ctx.resume().catch(() => { });
      }
      return this.ctx;
    }

    playNavTick() {
      const ctx = this.ensureContext();
      if (!ctx) return;
      const now = ctx.currentTime;
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(520, now);
      osc.frequency.exponentialRampToValueAtTime(780, now + 0.05);
      gain.gain.setValueAtTime(0.06, now);
      gain.gain.exponentialRampToValueAtTime(0.001, now + 0.06);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now);
      osc.stop(now + 0.065);
    }

    playLaunch() {
      this.playLaunchSwell();
    }

    playLaunchSwell() {
      const ctx = this.ensureContext();
      if (!ctx) return;
      const now = ctx.currentTime;
      [261.63, 329.63, 392.0, 523.25].forEach((freq, idx) => {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'triangle';
        osc.frequency.setValueAtTime(freq, now + idx * 0.04);
        gain.gain.setValueAtTime(0.05, now + idx * 0.04);
        gain.gain.exponentialRampToValueAtTime(0.001, now + 0.5);
        osc.connect(gain).connect(ctx.destination);
        osc.start(now + idx * 0.04);
        osc.stop(now + 0.52);
      });
    }

    playToastChime() {
      const ctx = this.ensureContext();
      if (!ctx) return;
      const now = ctx.currentTime;
      [587.33, 880.0].forEach((freq, i) => {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = 'sine';
        osc.frequency.setValueAtTime(freq, now + i * 0.07);
        gain.gain.setValueAtTime(0.05, now + i * 0.07);
        gain.gain.exponentialRampToValueAtTime(0.001, now + i * 0.07 + 0.24);
        osc.connect(gain).connect(ctx.destination);
        osc.start(now + i * 0.07);
        osc.stop(now + i * 0.07 + 0.26);
      });
    }
  }

  const sound = new Ps5SoundEngine();

  // --------------------------------------------------------------------------
  // 3. Application State (100% Real Data)
  // --------------------------------------------------------------------------
  let launcherSettings = {
    selected_wallpaper: 'assets/wallpapers/wall_1.jpeg',
    wallpaper_mode: 'game_hero_priority', // "wallpaper_priority" | "game_hero_priority"
    start_fullscreen: true,
    window_mode: 'fullscreen',
    target_monitor: 'primary',
    minimize_to_tray: false,
    on_game_launch_action: 'keep_open',
    close_button_action: 'exit_app',
    launch_on_windows_startup: false,
    enable_ui_sounds: true,
    ui_volume_percent: 80,
    enable_boot_video: true,
    enable_particles: false,
    show_fps_counter: true,
    show_ram_in_topbar: true,
    custom_wallpapers: [],
    last_update_check_ts: 0,
    latest_known_version: '',
    latest_release_url: '',
  };

  let activeGameRunning = false;
  let latestMobilePairingInfo = null;
  let activeConfirmResolver = null;
  let exitConfirmOpen = false;
  let latestAppUpdateInfo = {
    current_version: '1.0.0',
    latest_version: '',
    has_update: false,
    release_url: 'https://github.com/RashminaFdo/playcraft/releases/latest',
  };
  let updateBannerDismissed = false;

  let availableWallpapers = [
    { id: 'wall-1', name: 'Cosmic Singularity Vortex', url: 'assets/wallpapers/wall_1.jpeg', subtitle: '4K Wallpaper 1' },
    { id: 'wall-2', name: 'Planetary Cyber Grid', url: 'assets/wallpapers/wall_2.jpeg', subtitle: '4K Wallpaper 2' },
    { id: 'wall-3', name: 'Ancient Cyber Monolith', url: 'assets/wallpapers/wall_3.jpeg', subtitle: '4K Wallpaper 3' },
    { id: 'wall-4', name: 'Neon Night Cityscape', url: 'assets/wallpapers/wall_4.jpeg', subtitle: '4K Wallpaper 4' },
    { id: 'wall-5', name: 'Mecha Cyber Foundry', url: 'assets/wallpapers/wall_5.jpeg', subtitle: '4K Wallpaper 5' },
  ];

  let allGames = [];
  let filteredGames = [];
  let localMediaItems = [];
  let displayModesList = [];
  let currentTab = 'games';
  let selectedIndex = 0;
  let activeBgLayer = 'A';
  let toastTimeout = null;
  let selectedOnlineSteamAppId = null;
  let currentStoreFilter = 'ALL';
  let currentSortOrder = 'recent';

  function cleanDisplayText(str) {
    if (!str) return '';
    return String(str)
      .replace(/([a-z0-9])TM\b/g, '$1™')
      .replace(/&reg;/gi, '®')
      .replace(/&trade;/gi, '™')
      .replace(/&amp;/gi, '&');
  }

  function applyFiltersAndSort() {
    const q = (document.getElementById('searchInput')?.value || '').trim().toLowerCase();
    let list = allGames.filter((g) => {
      if (q) {
        const matchText =
          g.title.toLowerCase().includes(q) ||
          (g.genre && g.genre.toLowerCase().includes(q)) ||
          (g.exe_path && g.exe_path.toLowerCase().includes(q));
        if (!matchText) return false;
      }
      if (currentStoreFilter === 'FAV') return Boolean(g.is_favorite);
      if (currentStoreFilter === 'STEAM') return String(g.source) === 'Steam';
      if (currentStoreFilter === 'EPIC GAMES') return String(g.source) === 'Epic';
      if (currentStoreFilter === 'LOCAL') {
        const s = String(g.source);
        return s === 'Executable' || s === 'BatchScript' || s === 'Shortcut';
      }
      return true;
    });

    list.sort((a, b) => {
      // Keep favorites pinned first in default mode
      if (currentSortOrder === 'recent' && Boolean(a.is_favorite) !== Boolean(b.is_favorite)) {
        return a.is_favorite ? -1 : 1;
      }
      if (currentSortOrder === 'playtime') {
        return (Number(b.playtime_hours) || 0) - (Number(a.playtime_hours) || 0);
      }
      if (currentSortOrder === 'title') {
        return a.title.localeCompare(b.title);
      }
      if (currentSortOrder === 'size') {
        return (Number(b.size_on_disk_gb) || 0) - (Number(a.size_on_disk_gb) || 0);
      }
      return 0;
    });

    filteredGames = list;
  }

  function getMediaCatalog() {
    const mediaActivities = localMediaItems.length > 0
      ? localMediaItems.slice(0, 4).map((m, i) => ({
        id: `media-item-${i}`,
        title: m.name,
        subtitle: `${m.source_app} • ${m.size_mb} MB (Click to open in Explorer)`,
        tag: m.media_type,
        image_url: m.media_type === 'SCREENSHOT' ? `file:///${m.file_path.replace(/\\/g, '/')}` : (launcherSettings.selected_wallpaper || 'assets/wallpapers/wall_1.jpeg'),
        open_path: m.file_path,
      }))
      : availableWallpapers.slice(0, 4).map((w) => ({
        id: `wall-act-${w.id}`,
        title: w.name,
        subtitle: 'Click to apply this 4K wallpaper to your launcher',
        tag: 'WALLPAPER',
        image_url: w.url,
        wallpaper_url: w.url,
      }));

    return [
      {
        id: 'media-explore-hub',
        title: 'Medal.tv & Local Captures Hub',
        subtitle: `Detected ${localMediaItems.length} Local Clips & Screenshots | Rashmina Fernando`,
        description:
          'Launch Medal.tv directly for game recording, browse local gameplay clips and screenshots from your Medal & Windows Captures folders, or switch 4K launcher wallpapers.',
        source: 'PlayCraftHub',
        exe_path: '',
        working_dir: '',
        genre: 'Medal.tv & Local Media',
        cover_url: 'assets/playcraft-logo.jpg',
        hero_url: launcherSettings.selected_wallpaper || 'assets/wallpapers/wall_1.jpeg',
        playtime_hours: 0.0,
        last_played: 'Ready',
        size_on_disk_gb: 0.0,
        deep_optimize_before_launch: false,
        drm_bypass: {},
        activities: mediaActivities,
      },
    ];
  }

  function normalizeGame(g) {
    const srcRaw = String(g.source || 'Executable');
    let badge = srcRaw.toUpperCase();
    if (srcRaw === 'BatchScript') badge = 'SILENT .BAT';
    else if (srcRaw === 'Shortcut') badge = 'SHORTCUT .LNK';
    else if (srcRaw === 'Executable') badge = 'LOCAL .EXE';
    else if (srcRaw === 'Steam') badge = 'STEAM';
    else if (srcRaw === 'Epic') badge = 'EPIC GAMES';
    else if (srcRaw === 'Gog') badge = 'GOG GALAXY';
    else if (srcRaw === 'Ubisoft') badge = 'UBISOFT';
    else if (srcRaw === 'PlayCraftHub') badge = 'MEDAL & MEDIA';

    const drm = g.drm_bypass || {};
    return {
      ...g,
      sourceBadge: badge,
      bannerUrl: g.hero_url || launcherSettings.selected_wallpaper,
      coverUrl: g.cover_url || 'assets/playcraft-logo.jpg',
      sizeGb: g.size_on_disk_gb ?? 0.0,
      playtimeHrs: g.playtime_hours ?? 0.0,
      lastPlayedText: g.last_played || 'Never Played',
      deepOpt: g.deep_optimize_before_launch ?? true,
      drmNorm: {
        enabled: Boolean(drm.enabled),
        resolve_shipping_binary: Boolean(drm.force_direct_shipping_exe ?? true),
        bypass_steam_stub: Boolean(drm.bypass_steam_stub),
        bypass_epic_portal: Boolean(drm.bypass_epic_portal),
        force_offline_env: Boolean(drm.offline_mode_env),
        high_priority_process: Boolean(drm.high_priority_process ?? true),
        custom_launch_args: drm.custom_args || g.launch_args || '',
      },
    };
  }

  // --------------------------------------------------------------------------
  // 4. Startup Boot Intro Sequence (boot_intro.mp4)
  // --------------------------------------------------------------------------
  function initBootSequence() {
    const overlay = document.getElementById('bootIntroOverlay');
    const video = document.getElementById('bootIntroVideo');
    const skipBtn = document.getElementById('skipBootBtn');
    const replayBtn = document.getElementById('replayBootBtn');

    function dismissBoot() {
      if (!overlay || overlay.classList.contains('fade-out')) return;
      sound.playLaunchSwell();
      overlay.classList.add('fade-out');
      setTimeout(() => {
        if (video) video.pause();
      }, 450);
    }
    window.__playcraftDismissBoot = dismissBoot;

    if (!launcherSettings.enable_boot_video) {
      if (overlay) overlay.classList.add('fade-out');
    } else if (video) {
      video.muted = false;
      video.volume = 1.0;
      video.addEventListener('ended', dismissBoot);
      video.addEventListener('error', dismissBoot);

      // Safety timeout: automatically fade out boot intro after 7.5 seconds max
      setTimeout(dismissBoot, 7500);

      const playPromise = video.play();
      if (playPromise !== undefined) {
        playPromise.catch((err) => {
          console.warn('Initial unmuted boot video autoplay policy caught:', err);
          const unlockAudio = () => {
            video.muted = false;
            video.volume = 1.0;
            video.play().catch(() => { });
            window.removeEventListener('pointerdown', unlockAudio);
            window.removeEventListener('keydown', unlockAudio);
          };
          window.addEventListener('pointerdown', unlockAudio);
          window.addEventListener('keydown', unlockAudio);
          // If strictly blocked, start muted to avoid freeze, but unlock immediately on interaction
          video.muted = true;
          video.play().catch(() => setTimeout(dismissBoot, 2000));
        });
      }
    }

    if (skipBtn) skipBtn.addEventListener('click', dismissBoot);
    if (overlay) overlay.addEventListener('click', dismissBoot);

    if (replayBtn && overlay && video) {
      replayBtn.addEventListener('click', () => {
        closeTopLayer(document.getElementById('settingsDialog'));
        overlay.classList.remove('fade-out');
        video.currentTime = 0;
        video.muted = false;
        video.volume = 1.0;
        video.play().catch(() => {
          video.muted = false;
          video.play().catch(() => { });
        });
      });
    }

    setTimeout(dismissBoot, 6200);
  }

  // --------------------------------------------------------------------------
  // 5. Toast Notification Helper
  // --------------------------------------------------------------------------
  function showToast(title, message) {
    const toast = document.getElementById('ps5Toast');
    const tTitle = document.getElementById('toastTitle');
    const tMsg = document.getElementById('toastMessage');
    if (!toast || !tTitle || !tMsg) return;

    tTitle.textContent = title;
    tMsg.textContent = message;
    toast.classList.remove('hidden');
    sound.playToastChime();

    if (toastTimeout) clearTimeout(toastTimeout);
    toastTimeout = setTimeout(() => {
      toast.classList.add('hidden');
    }, 4000);
  }

  // --------------------------------------------------------------------------
  // 6. Wallpaper & Settings Application
  // --------------------------------------------------------------------------
  function getActiveWallpaperUrl() {
    return launcherSettings.selected_wallpaper || 'assets/wallpapers/wall_1.jpeg';
  }

  function isWallpaperLocked() {
    return launcherSettings.wallpaper_mode === 'wallpaper_priority';
  }

  function setHeroBackgroundUrl(url) {
    const bgA = document.getElementById('heroBgA');
    const bgB = document.getElementById('heroBgB');
    const fallback = getActiveWallpaperUrl();
    const targetUrl = isWallpaperLocked() ? fallback : url || fallback;

    if (bgA && bgB) {
      if (activeBgLayer === 'A') {
        bgB.style.backgroundImage = `url('${targetUrl}'), url('${fallback}')`;
        bgB.classList.add('active');
        bgA.classList.remove('active');
        activeBgLayer = 'B';
      } else {
        bgA.style.backgroundImage = `url('${targetUrl}'), url('${fallback}')`;
        bgA.classList.add('active');
        bgB.classList.remove('active');
        activeBgLayer = 'A';
      }
    }
  }

  function applySettingsToUi() {
    sound.enabled = Boolean(launcherSettings.enable_ui_sounds);

    const fpsPill = document.getElementById('fpsCounter');
    if (fpsPill) {
      fpsPill.style.display = launcherSettings.show_fps_counter ? '' : 'none';
    }

    const setCheck = (id, val) => {
      const el = document.getElementById(id);
      if (el) el.checked = Boolean(val);
    };

    setCheck('settingLockWallpaper', isWallpaperLocked());
    setCheck('settingStartFullscreen', launcherSettings.start_fullscreen);
    setCheck('settingMinimizeToTray', launcherSettings.minimize_to_tray);
    setCheck('settingLaunchOnStartup', launcherSettings.launch_on_windows_startup);
    setCheck('settingShowFps', launcherSettings.show_fps_counter);
    setCheck('settingUiSounds', launcherSettings.enable_ui_sounds);
    setCheck('settingBootVideo', launcherSettings.enable_boot_video);

    const winModeEl = document.getElementById('settingWindowMode');
    if (winModeEl) {
      winModeEl.value = launcherSettings.window_mode || (launcherSettings.start_fullscreen ? 'fullscreen' : 'windowed');
    }

    const monEl = document.getElementById('settingTargetMonitor');
    if (monEl) {
      monEl.value = launcherSettings.target_monitor || 'primary';
    }

    const gameLaunchEl = document.getElementById('settingOnGameLaunch');
    if (gameLaunchEl) gameLaunchEl.value = launcherSettings.on_game_launch_action || 'keep_open';

    const closeBtnEl = document.getElementById('settingCloseButtonAction');
    if (closeBtnEl) closeBtnEl.value = launcherSettings.close_button_action || 'exit_app';

    renderWallpaperGallery();
    const list = getActiveList();
    if (list.length > 0 && list[selectedIndex]) {
      setHeroBackgroundUrl(list[selectedIndex].bannerUrl);
    } else {
      setHeroBackgroundUrl(getActiveWallpaperUrl());
    }
  }

  async function persistSettings() {
    try {
      await tauriInvoke('save_launcher_settings', { settings: launcherSettings });
    } catch (_) { }
  }

  function renderWallpaperGallery() {
    const grid = document.getElementById('wallpaperGalleryGrid');
    if (!grid) return;

    const currentUrl = getActiveWallpaperUrl();
    grid.innerHTML = availableWallpapers
      .map((w) => {
        const isSelected = currentUrl === w.url;
        return `
          <div class="wallpaper-card ${isSelected ? 'selected' : ''}" data-wall-url="${w.url}">
            <img src="${w.url}" alt="${w.name}" />
            <div class="wallpaper-card-label">
              <span>${w.name}</span>
              ${isSelected ? '<span class="wallpaper-active-pill">ACTIVE</span>' : ''}
            </div>
          </div>
        `;
      })
      .join('');

    grid.querySelectorAll('.wallpaper-card').forEach((card) => {
      card.addEventListener('click', async () => {
        const url = card.getAttribute('data-wall-url');
        if (!url) return;
        launcherSettings.selected_wallpaper = url;
        sound.playNavTick();
        applySettingsToUi();
        await persistSettings();
        showToast('Wallpaper Updated', `Applied ${url.split('/').pop()} to PlayCraft.`);
      });
    });
  }

  // --------------------------------------------------------------------------
  // 7. Render Horizontal Game Tile Strip & Focused Hero View
  // --------------------------------------------------------------------------
  function getActiveList() {
    if (currentTab === 'media') return getMediaCatalog().map(normalizeGame);
    return filteredGames.map(normalizeGame);
  }

  function renderTileStrip() {
    const strip = document.getElementById('gameTileStrip');
    if (!strip) return;

    const list = getActiveList();
    if (selectedIndex >= list.length) selectedIndex = 0;

    strip.innerHTML = '';

    list.forEach((game, idx) => {
      const tile = document.createElement('button');
      tile.type = 'button';
      tile.className = 'ps5-tile' + (idx === selectedIndex ? ' selected' : '');
      tile.setAttribute('data-index', String(idx));
      const cleanTitle = cleanDisplayText(game.title);
      tile.setAttribute('aria-label', cleanTitle);
      if (idx === selectedIndex) {
        tile.setAttribute('aria-current', 'true');
      }

      tile.innerHTML = `
        <div class="tile-art-wrap">
          <img src="${game.coverUrl}" onerror="this.src='assets/playcraft-logo.jpg'" alt="${cleanTitle}" />
          ${game.is_favorite ? '<span class="tile-fav-badge" title="Pinned Favorite">★</span>' : ''}
        </div>
        <span class="tile-caption">${cleanTitle}</span>
      `;

      tile.addEventListener('click', () => {
        selectGameIndex(idx, true);
      });

      tile.addEventListener('dblclick', () => {
        selectGameIndex(idx, false);
        launchSelectedGame();
      });

      tile.addEventListener('contextmenu', (ev) => {
        ev.preventDefault();
        selectGameIndex(idx, true);
        openTileContextMenu(ev.clientX, ev.clientY, game);
      });

      strip.appendChild(tile);
    });

    const utilityTiles = [
      {
        label: 'Add Game',
        svg: '<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"><path d="M12 5v14M5 12h14"/></svg>',
        action: () => openModal('scannerDialog'),
      },
      {
        label: 'Open Medal',
        svg: '<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"><polygon points="23 7 16 12 23 17 23 7"/><rect width="15" height="14" x="1" y="5" rx="2" ry="2"/></svg>',
        action: () => triggerOpenMedal(),
      },
      {
        label: 'Mobile Pad',
        svg: '<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 12h4m-2-2v4m7-1h.01M18 11h.01"/></svg>',
        action: () => openModal('mobileDialog'),
      },
      {
        label: 'Settings',
        svg: '<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c.26.604.852.997 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>',
        action: () => openModal('settingsDialog'),
      },
    ];

    utilityTiles.forEach((u) => {
      const uTile = document.createElement('button');
      uTile.type = 'button';
      uTile.className = 'ps5-tile' + (list.length === 0 && u.label === 'Add Game' ? ' selected' : '');
      uTile.title = u.label;
      uTile.innerHTML = `
        <div class="tile-art-wrap">
          <div class="tile-icon-glyph">${u.svg}</div>
        </div>
        <span class="tile-caption">${u.label}</span>
      `;
      uTile.addEventListener('click', u.action);
      strip.appendChild(uTile);
    });

    if (list.length > 0) {
      updateHeroSpotlight(list[selectedIndex]);
    } else {
      renderEmptyLibrarySpotlight();
    }
  }

  function renderEmptyLibrarySpotlight() {
    setHeroBackgroundUrl(getActiveWallpaperUrl());

    const badge = document.getElementById('heroSourceBadge');
    const subtitle = document.getElementById('heroSubtitle');
    const title = document.getElementById('heroTitle');
    const desc = document.getElementById('heroDescription');
    const playtime = document.getElementById('statPlaytime');
    const lastPlayed = document.getElementById('statLastPlayed');
    const size = document.getElementById('statSize');
    const playLabel = document.getElementById('playBtnLabel');
    const removeBtn = document.getElementById('removeGameBtn');
    const wsBtn = document.getElementById('openGameWorkspaceBtn');
    const folderBtn = document.getElementById('openInstallFolderBtn');
    const actRow = document.getElementById('activityCardsRow');

    if (badge) badge.textContent = 'LIBRARY EMPTY';
    if (subtitle) subtitle.textContent = 'PlayCraft by Rashmina Fernando (www.rashmina.dev)';
    if (title) title.textContent = 'Add Your First Game';
    if (desc) {
      desc.textContent =
        'No fake or placeholder games are preloaded. Click "Add Game" to pick any .exe, .bat, or .lnk file (with automatic online database cover art & description matching) or sync your Steam, Epic Games, GOG, and Ubisoft libraries.';
    }
    if (playtime) playtime.textContent = '0.0 hrs';
    if (lastPlayed) lastPlayed.textContent = 'Never';
    if (size) size.textContent = '0.0 GB';
    if (playLabel) playLabel.textContent = 'Add Game';
    if (removeBtn) removeBtn.classList.add('hidden');
    if (wsBtn) wsBtn.classList.add('hidden');
    if (folderBtn) folderBtn.classList.add('hidden');
    if (actRow) actRow.innerHTML = '';
  }

  function selectGameIndex(newIndex, playSound = true) {
    const list = getActiveList();
    if (!list.length) return;
    const clamped = (newIndex + list.length) % list.length;
    selectedIndex = clamped;
    if (playSound) sound.playNavTick();

    const strip = document.getElementById('gameTileStrip');
    if (strip) {
      const tiles = strip.querySelectorAll('.ps5-tile[data-index]');
      tiles.forEach((t, idx) => {
        if (idx === selectedIndex) {
          t.classList.add('selected');
          t.setAttribute('aria-current', 'true');
          t.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'center' });
        } else {
          t.classList.remove('selected');
          t.removeAttribute('aria-current');
        }
      });
    }

    updateHeroSpotlight(list[selectedIndex]);
  }

  function updateHeroSpotlight(game) {
    if (!game) {
      renderEmptyLibrarySpotlight();
      return;
    }

    setHeroBackgroundUrl(game.bannerUrl);

    const badge = document.getElementById('heroSourceBadge');
    const genreBadge = document.getElementById('heroGenreBadge');
    const subtitle = document.getElementById('heroSubtitle');
    const title = document.getElementById('heroTitle');
    const favBtn = document.getElementById('favoriteGameBtn');
    const desc = document.getElementById('heroDescription');
    const playtime = document.getElementById('statPlaytime');
    const lastPlayed = document.getElementById('statLastPlayed');
    const size = document.getElementById('statSize');
    const playLabel = document.getElementById('playBtnLabel');
    const removeBtn = document.getElementById('removeGameBtn');
    const wsBtn = document.getElementById('openGameWorkspaceBtn');
    const folderBtn = document.getElementById('openInstallFolderBtn');

    if (badge) badge.textContent = game.sourceBadge || 'LOCAL .EXE';
    if (genreBadge) {
      genreBadge.textContent = (game.genre || 'PC GAME').toUpperCase();
      genreBadge.classList.remove('hidden');
    }
    if (subtitle) {
      const hzTag = game.target_refresh_hz ? ` • ${game.target_refresh_hz}Hz` : '';
      const profTag = game.active_launch_profile && game.active_launch_profile !== 'Default' ? ` • [${game.active_launch_profile}]` : '';
      subtitle.textContent = (game.subtitle || game.exe_path || '') + hzTag + profTag;
    }
    if (title) title.textContent = cleanDisplayText(game.title);
    if (desc) desc.textContent = cleanDisplayText(game.description);
    if (playtime) playtime.textContent = `${Number(game.playtimeHrs || 0).toFixed(1)} hrs`;
    if (lastPlayed) lastPlayed.textContent = game.lastPlayedText || 'Never Played';
    if (size) size.textContent = `${Number(game.sizeGb || 0).toFixed(2)} GB`;

    const isMediaHub = game.id === 'media-explore-hub';
    if (favBtn) {
      favBtn.classList.toggle('hidden', isMediaHub || currentTab !== 'games');
      favBtn.classList.toggle('favorited', Boolean(game.is_favorite));
    }
    if (playLabel) {
      if (isMediaHub) {
        playLabel.textContent = 'Open Medal.tv';
      } else {
        playLabel.textContent =
          game.sourceBadge === 'SILENT .BAT' ? 'Launch (.BAT)' : 'Play Game';
      }
    }

    if (removeBtn) removeBtn.classList.toggle('hidden', isMediaHub || currentTab !== 'games');
    if (wsBtn) wsBtn.classList.toggle('hidden', isMediaHub);
    if (folderBtn) folderBtn.classList.toggle('hidden', isMediaHub);

    // Sync Overlay Notes
    const ovNotes = document.getElementById('ovGameNotesContent');
    if (ovNotes) {
      ovNotes.textContent =
        game.notes && game.notes.trim()
          ? game.notes
          : `No notes saved for ${game.title}. Click "Saves, Hz & Notes" to add notes.`;
    }

    syncDrmModalWithGame(game);

    const actRow = document.getElementById('activityCardsRow');
    if (actRow) {
      const cards = game.activities && game.activities.length > 0 ? game.activities : [];
      actRow.innerHTML = cards
        .slice(0, 4)
        .map(
          (c, i) => `
        <article class="activity-card" data-activity-id="${c.id || i}" data-wall-url="${c.wallpaper_url || ''}" data-open-path="${(c.open_path || '').replace(/"/g, '&quot;')}">
          <div class="activity-card-overlay">
            <span class="activity-tag">${c.tag}</span>
            <div>
              <div class="activity-title">${cleanDisplayText(c.title)}</div>
              <div class="activity-sub">${cleanDisplayText(c.subtitle)}</div>
            </div>
          </div>
        </article>
      `
        )
        .join('');

      actRow.querySelectorAll('.activity-card').forEach((cardEl) => {
        cardEl.addEventListener('click', async () => {
          const wallUrl = cardEl.getAttribute('data-wall-url');
          const openPath = cardEl.getAttribute('data-open-path');
          if (openPath) {
            try {
              await tauriInvoke('open_path_in_explorer', { targetPath: openPath });
            } catch (e) {
              showToast('Explorer Notice', String(e));
            }
          } else if (wallUrl) {
            launcherSettings.selected_wallpaper = wallUrl;
            applySettingsToUi();
            await persistSettings();
            showToast('Wallpaper Applied', `Selected ${wallUrl.split('/').pop()}`);
          } else {
            launchSelectedGame();
          }
        });
      });
    }
  }

  // --------------------------------------------------------------------------
  // 7B. Desktop Context Menu, Favorites & Full All-Games Grid Drawer
  // --------------------------------------------------------------------------
  function openTileContextMenu(x, y, game) {
    const menu = document.getElementById('tileContextMenu');
    const titleEl = document.getElementById('ctxMenuGameTitle');
    if (!menu || !game || game.id === 'media-explore-hub') return;

    if (titleEl) titleEl.textContent = cleanDisplayText(game.title);
    menu.classList.remove('hidden');

    // Clamp to viewport bounds
    const menuW = 240;
    const menuH = 260;
    const safeX = Math.min(window.innerWidth - menuW - 12, Math.max(12, x));
    const safeY = Math.min(window.innerHeight - menuH - 12, Math.max(12, y));
    menu.style.left = `${safeX}px`;
    menu.style.top = `${safeY}px`;
  }

  function closeTileContextMenu() {
    document.getElementById('tileContextMenu')?.classList.add('hidden');
  }

  async function toggleFavoriteSelectedGame() {
    const list = getActiveList();
    const game = list[selectedIndex];
    if (!game || game.id === 'media-explore-hub') return;

    try {
      const updated = await tauriInvoke('toggle_favorite_game', { gameId: game.id });
      if (Array.isArray(updated)) {
        allGames = updated;
        applyFiltersAndSort();
        const newIdx = filteredGames.findIndex((g) => g.id === game.id);
        if (newIdx >= 0) selectedIndex = newIdx;
        renderTileStrip();
        const nowFav = allGames.find((g) => g.id === game.id)?.is_favorite;
        showToast(
          nowFav ? 'Pinned to Favorites ★' : 'Unpinned from Favorites',
          `${cleanDisplayText(game.title)} ${nowFav ? 'is now pinned to the front of your library.' : 'was unpinned.'}`
        );
      }
    } catch (e) {
      showToast('Favorite Notice', String(e));
    }
  }

  function renderLibraryFullGrid() {
    const grid = document.getElementById('libraryFullGridContainer');
    const subtitle = document.getElementById('gridLibraryCountSubtitle');
    if (!grid) return;

    const games = filteredGames.map(normalizeGame);
    if (subtitle) {
      subtitle.textContent = `Showing ${games.length} of ${allGames.length} installed games • Click any card to select or Play`;
    }

    if (!games.length) {
      grid.innerHTML = `<div class="db-empty-hint" style="grid-column:1/-1">No games match the current filter. Click "+ Add Game" to add titles.</div>`;
      return;
    }

    grid.innerHTML = games
      .map((g, idx) => {
        const cleanTitle = cleanDisplayText(g.title);
        return `
          <div class="lib-grid-card" data-grid-index="${idx}">
            <img src="${g.coverUrl}" onerror="this.onerror=null;this.src='assets/playcraft-logo.jpg'" alt="${cleanTitle}" />
            <div class="lib-grid-overlay">
              <div class="lib-grid-title">${g.is_favorite ? '★ ' : ''}${cleanTitle}</div>
              <div class="lib-grid-meta">
                <span>${g.sourceBadge}</span>
                <span>${Number(g.sizeGb || 0).toFixed(1)} GB</span>
              </div>
            </div>
          </div>
        `;
      })
      .join('');

    grid.querySelectorAll('.lib-grid-card').forEach((card) => {
      card.addEventListener('click', () => {
        const idx = parseInt(card.getAttribute('data-grid-index'), 10);
        if (!Number.isNaN(idx)) {
          currentTab = 'games';
          selectGameIndex(idx, true);
          closeTopLayer(document.getElementById('libraryGridDialog'));
        }
      });
      card.addEventListener('dblclick', () => {
        const idx = parseInt(card.getAttribute('data-grid-index'), 10);
        if (!Number.isNaN(idx)) {
          currentTab = 'games';
          selectGameIndex(idx, false);
          closeTopLayer(document.getElementById('libraryGridDialog'));
          launchSelectedGame();
        }
      });
    });
  }

  // --------------------------------------------------------------------------
  // 8. Scroll-Snap State Sync (modern-web-guidance: scroll-snap-state-sync)
  // --------------------------------------------------------------------------
  function setupTileStripScrollSync() {
    const strip = document.getElementById('gameTileStrip');
    if (!strip) return;

    strip.addEventListener('scrollsnapchange', (event) => {
      const snappedTile = event.snapTargetInline;
      if (snappedTile && snappedTile.hasAttribute('data-index')) {
        const idx = parseInt(snappedTile.getAttribute('data-index'), 10);
        if (!Number.isNaN(idx)) selectGameIndex(idx, true);
      }
    });

    if (!('onscrollsnapchange' in HTMLElement.prototype)) {
      const observer = new IntersectionObserver(
        (entries) => {
          entries.forEach((entry) => {
            if (entry.isIntersecting && entry.intersectionRatio >= 0.9) {
              // Scroll-snap fallback observer
            }
          });
        },
        { root: strip, threshold: 0.9 }
      );
      strip.querySelectorAll('.ps5-tile').forEach((t) => observer.observe(t));
    }
  }

  // --------------------------------------------------------------------------
  // 9. Online Game Database Auto-Match (Steam Storefront Keyless API via Rust)
  // --------------------------------------------------------------------------
  async function searchAndAutoFillGameMetadata(queryText, autoApplyFirstMatch = true) {
    const resultsContainer = document.getElementById('dbSearchResults');
    const searchInput = document.getElementById('onlineDbSearchInput');
    if (searchInput && queryText) searchInput.value = queryText;

    const cleanQuery = (queryText || searchInput?.value || '').trim();
    if (!cleanQuery) return;

    if (resultsContainer) {
      resultsContainer.innerHTML = `<div class="db-empty-hint">Searching online game database for "${cleanQuery}"...</div>`;
    }

    try {
      const data = await tauriInvoke('search_online_game_db', { query: cleanQuery });
      const items = data && Array.isArray(data.items) ? data.items : [];

      if (!items.length) {
        if (resultsContainer) {
          resultsContainer.innerHTML = `<div class="db-empty-hint">No online matches found for "${cleanQuery}". You can save the game with its detected title.</div>`;
        }
        return;
      }

      if (resultsContainer) {
        resultsContainer.innerHTML = items
          .slice(0, 8)
          .map((item, idx) => {
            const appId = item.id;
            const thumb = item.tiny_image || `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/header.jpg`;
            const safeThumb = thumb.replace(/"/g, '&quot;');
            return `
              <button type="button" class="db-result-item ${idx === 0 && autoApplyFirstMatch ? 'active' : ''}" data-appid="${appId}" data-name="${(item.name || '').replace(/"/g, '&quot;')}" data-thumb="${safeThumb}">
                <img src="${safeThumb}" onerror="this.onerror=null;this.src='assets/playcraft-logo.jpg'" alt="${(item.name || '').replace(/"/g, '&quot;')}" />
                <div class="db-result-meta">
                  <div class="db-result-title">${item.name}</div>
                  <div class="db-result-sub">AppID ${appId}</div>
                </div>
              </button>
            `;
          })
          .join('');

        resultsContainer.querySelectorAll('.db-result-item').forEach((btn) => {
          btn.addEventListener('click', () => {
            resultsContainer.querySelectorAll('.db-result-item').forEach((b) => b.classList.remove('active'));
            btn.classList.add('active');
            const appId = parseInt(btn.getAttribute('data-appid'), 10);
            const name = btn.getAttribute('data-name');
            const thumb = btn.getAttribute('data-thumb');
            applyOnlineGameMatch(appId, name, thumb);
          });
        });
      }

      if (autoApplyFirstMatch && items[0]) {
        await applyOnlineGameMatch(items[0].id, items[0].name, items[0].tiny_image);
      }
    } catch (err) {
      if (resultsContainer) {
        resultsContainer.innerHTML = `<div class="db-empty-hint">Online lookup notice: ${String(err)}</div>`;
      }
    }
  }

  async function applyOnlineGameMatch(appId, fallbackName, fallbackThumb) {
    if (!appId) return;
    selectedOnlineSteamAppId = appId;

    const initialCover = fallbackThumb || `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/header.jpg`;
    const initialHero = `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/library_hero.jpg`;

    const titleEl = document.getElementById('customTitle');
    const coverEl = document.getElementById('customCoverUrl');
    const heroEl = document.getElementById('customHeroUrl');
    const previewImg = document.getElementById('previewCoverImg');

    if (titleEl && fallbackName) titleEl.value = fallbackName;
    if (coverEl) coverEl.value = initialCover;
    if (heroEl) heroEl.value = initialHero;
    if (previewImg) {
      previewImg.onerror = () => {
        previewImg.onerror = null;
        previewImg.src = fallbackThumb || 'assets/playcraft-logo.jpg';
      };
      previewImg.src = initialCover;
    }

    try {
      const detailsMap = await tauriInvoke('fetch_steam_app_details', { appId: Number(appId) });
      const entry = detailsMap && detailsMap[String(appId)];
      if (entry && entry.success && entry.data) {
        const d = entry.data;
        if (titleEl && d.name) titleEl.value = d.name;

        const bestCover = d.header_image || d.capsule_image || d.capsule_imagev5 || initialCover;
        const bestHero =
          d.background_raw ||
          (Array.isArray(d.screenshots) && d.screenshots[0]?.path_full) ||
          d.background ||
          bestCover;

        if (coverEl) coverEl.value = bestCover;
        if (heroEl) heroEl.value = bestHero;
        if (previewImg) {
          previewImg.onerror = () => {
            previewImg.onerror = null;
            previewImg.src = fallbackThumb || 'assets/playcraft-logo.jpg';
          };
          previewImg.src = bestCover;
        }

        const descEl = document.getElementById('customDescription');
        if (descEl && d.short_description) {
          const tmp = document.createElement('div');
          tmp.innerHTML = d.short_description;
          descEl.value = tmp.textContent || tmp.innerText || d.short_description;
        }

        const genreEl = document.getElementById('customGenre');
        if (genreEl && Array.isArray(d.genres) && d.genres.length > 0) {
          genreEl.value = d.genres.map((g) => g.description).slice(0, 3).join(' / ');
        }

        showToast('Metadata Auto-Filled', `Loaded official description, genre & cover art for ${d.name}.`);
      }
    } catch (_) { }
  }

  // --------------------------------------------------------------------------
  // 10. Real PC System Specifications Renderer
  // --------------------------------------------------------------------------
  async function loadAndRenderSystemSpecs() {
    const container = document.getElementById('fullSystemSpecsContainer');
    if (!container) return;

    try {
      const s = await tauriInvoke('get_full_system_specs');
      if (!s) return;

      const gpuHtml = (s.gpus || [])
        .map(
          (g, i) => `
          <div style="margin-top:${i > 0 ? '6px' : '0'}">
            <div class="spec-card-main">${g.name}</div>
            <div class="spec-card-sub">VRAM: ${g.vram_gb} GB • Driver: ${g.driver_version}</div>
          </div>
        `
        )
        .join('');

      const disksHtml = (s.disks || [])
        .map(
          (d) => `
          <div class="disk-item-row">
            <div style="display:flex;justify-content:space-between;font-size:0.82rem;font-weight:700;color:#fff;">
              <span>${d.mount_point} ${d.name ? `(${d.name})` : ''} — ${d.disk_type || 'Drive'} [${d.file_system}]</span>
              <span style="color:var(--pc-cyan)">${d.used_gb} GB / ${d.total_gb} GB (${d.usage_percent}%)</span>
            </div>
            <div class="spec-card-sub">Free Space: ${d.available_gb} GB</div>
            <div class="disk-bar-track">
              <div class="disk-bar-fill" style="width:${Math.min(100, d.usage_percent)}%"></div>
            </div>
          </div>
        `
        )
        .join('');

      container.innerHTML = `
        <div class="spec-card">
          <div class="spec-card-label">PROCESSOR (CPU)</div>
          <div class="spec-card-main">${s.cpu_brand}</div>
          <div class="spec-card-sub">
            ${s.physical_cores} Physical Cores / ${s.logical_threads} Logical Threads • ${s.cpu_frequency_mhz} MHz (${s.architecture})
          </div>
        </div>

        <div class="spec-card">
          <div class="spec-card-label">GRAPHICS ADAPTERS (GPU)</div>
          ${gpuHtml}
        </div>

        <div class="spec-card">
          <div class="spec-card-label">SYSTEM MEMORY (RAM & SWAP)</div>
          <div class="spec-card-main">${s.ram_total_gb} GB Total RAM</div>
          <div class="spec-card-sub">
            Used: ${s.ram_used_gb} GB (${s.ram_usage_percent}%) • Free: ${s.ram_available_gb} GB • Swap: ${s.swap_total_gb} GB
          </div>
        </div>

        <div class="spec-card">
          <div class="spec-card-label">MOTHERBOARD & DISPLAY</div>
          <div class="spec-card-main">${s.motherboard}</div>
          <div class="spec-card-sub">
            BIOS: ${s.bios_version} • Primary Monitor: ${s.primary_display_resolution}
          </div>
        </div>

        <div class="spec-card full-span">
          <div class="spec-card-label">OPERATING SYSTEM & HOST</div>
          <div class="spec-card-main">${s.os_name} (${s.os_version})</div>
          <div class="spec-card-sub">
            Host: ${s.hostname} • Kernel: ${s.kernel_version} • Uptime: ${s.uptime_formatted}
          </div>
        </div>

        <div class="spec-card full-span">
          <div class="spec-card-label">STORAGE VOLUMES (${(s.disks || []).length} DETECTED)</div>
          ${disksHtml || '<div class="spec-card-sub">No fixed disks detected</div>'}
        </div>
      `;
    } catch (err) {
      container.innerHTML = `<div class="spec-card full-span"><div class="spec-card-sub">Could not query hardware specs: ${String(err)}</div></div>`;
    }
  }

  // --------------------------------------------------------------------------
  // 11. Per-Game Workspace (Save Vault, Refresh Rate Hz, Notes & Mods)
  // --------------------------------------------------------------------------
  async function openGameWorkspaceModal() {
    const list = getActiveList();
    const game = list[selectedIndex];
    if (!game || game.id === 'media-explore-hub') return;

    document.getElementById('wsGameTitle').textContent = game.title;
    document.getElementById('wsSaveFolderPath').value = game.save_folder_path || '';
    document.getElementById('wsAutoBackupSaves').checked = Boolean(game.auto_backup_saves);
    document.getElementById('wsModFolderPath').value = game.mod_folder_path || game.working_dir || '';
    document.getElementById('wsGameNotes').value = game.notes || '';
    document.getElementById('wsLaunchProfileSelect').value = game.active_launch_profile || 'Default';
    const wsCover = document.getElementById('wsCoverUrlInput');
    const wsHero = document.getElementById('wsHeroUrlInput');
    if (wsCover) wsCover.value = game.cover_url || '';
    if (wsHero) wsHero.value = game.hero_url || '';

    // Populate Display Modes Dropdown
    const modeSelect = document.getElementById('wsDisplayModeSelect');
    if (modeSelect) {
      if (!displayModesList.length) {
        try {
          displayModesList = (await tauriInvoke('get_available_display_modes')) || [];
        } catch (_) { }
      }
      const currentVal =
        game.target_resolution && game.target_refresh_hz
          ? `${game.target_resolution}@${game.target_refresh_hz}`
          : '';
      modeSelect.innerHTML =
        `<option value="">Keep Desktop Default</option>` +
        displayModesList
          .slice(0, 28)
          .map((m) => {
            const val = `${m.width}x${m.height}@${m.refresh_hz}`;
            return `<option value="${val}" ${val === currentVal ? 'selected' : ''}>${m.label}</option>`;
          })
          .join('');
    }

    await refreshGameSaveBackupsList(game.id);
    openModal('workspaceDialog');
  }

  async function refreshGameSaveBackupsList(gameId) {
    const container = document.getElementById('wsBackupsList');
    if (!container) return;
    try {
      const backups = await tauriInvoke('list_save_backups', { gameId });
      if (!Array.isArray(backups) || backups.length === 0) {
        container.innerHTML = `<div class="db-empty-hint">No snapshots created for this game yet.</div>`;
        return;
      }
      container.innerHTML = backups
        .map(
          (b) => `
          <div class="backup-row">
            <div>
              <strong style="color:#fff">${b.timestamp}</strong>
              <span style="color:var(--ps5-muted);margin-left:8px;font-family:var(--font-mono);font-size:0.74rem;">${b.file_count} files (${b.size_mb} MB)</span>
            </div>
            <button type="button" class="small-cyan-btn" data-restore-id="${b.id}">Restore</button>
          </div>
        `
        )
        .join('');

      container.querySelectorAll('[data-restore-id]').forEach((btn) => {
        btn.addEventListener('click', async () => {
          const backupId = btn.getAttribute('data-restore-id');
          try {
            const msg = await tauriInvoke('restore_save_backup', { gameId, backupId });
            showToast('Save Snapshot Restored', msg);
          } catch (e) {
            showToast('Restore Error', String(e));
          }
        });
      });
    } catch (_) { }
  }

  // --------------------------------------------------------------------------
  // 12. Playtime Analytics & Session History Modal
  // --------------------------------------------------------------------------
  async function openAnalyticsModal() {
    const cardsEl = document.getElementById('analyticsSummaryCards');
    const tableWrap = document.getElementById('sessionHistoryTableWrap');

    const totalGames = allGames.length;
    const totalHours = allGames.reduce((acc, g) => acc + (Number(g.playtime_hours) || 0), 0);
    const totalDiskGb = allGames.reduce((acc, g) => acc + (Number(g.size_on_disk_gb) || 0), 0);

    let sessions = [];
    try {
      sessions = (await tauriInvoke('get_session_history')) || [];
    } catch (_) { }

    if (cardsEl) {
      cardsEl.innerHTML = `
        <div class="spec-card">
          <div class="spec-card-label">INSTALLED GAMES</div>
          <div class="spec-card-main">${totalGames} Titles</div>
          <div class="spec-card-sub">Across Steam, Epic, GOG & Local</div>
        </div>
        <div class="spec-card">
          <div class="spec-card-label">TOTAL PLAYTIME</div>
          <div class="spec-card-main">${totalHours.toFixed(1)} hrs</div>
          <div class="spec-card-sub">100% Real Tracked Playtime</div>
        </div>
        <div class="spec-card">
          <div class="spec-card-label">LIBRARY FOOTPRINT</div>
          <div class="spec-card-main">${totalDiskGb.toFixed(1)} GB</div>
          <div class="spec-card-sub">Verified Local Disk Usage</div>
        </div>
        <div class="spec-card">
          <div class="spec-card-label">RECORDED SESSIONS</div>
          <div class="spec-card-main">${sessions.length} Sessions</div>
          <div class="spec-card-sub">Logged in playcraft_sessions.json</div>
        </div>
      `;
    }

    if (tableWrap) {
      if (!sessions.length) {
        tableWrap.innerHTML = `<div class="db-empty-hint">No gameplay sessions recorded yet. Launch any game to automatically record session duration.</div>`;
      } else {
        tableWrap.innerHTML = sessions
          .slice(0, 30)
          .map(
            (s) => `
            <div class="session-row">
              <div>
                <strong style="color:#fff">${s.game_title}</strong>
                <span class="badge-cyan" style="margin-left:8px">${s.store_source}</span>
              </div>
              <div style="font-family:var(--font-mono);color:var(--pc-cyan);font-weight:700;">
                ${s.duration_minutes} mins • ${s.started_at}
              </div>
            </div>
          `
          )
          .join('');
      }
    }

    openModal('analyticsDialog');
  }

  async function triggerOpenMedal() {
    try {
      const msg = await tauriInvoke('launch_medal_app');
      showToast('Medal.tv Recorder', msg);
    } catch (e) {
      showToast('Medal Notice', String(e));
    }
  }

  // --------------------------------------------------------------------------
  // 13. Native Tauri v2 Commands & Event Listeners
  // --------------------------------------------------------------------------
  async function loadInitialBackendData() {
    try {
      const savedSettings = await tauriInvoke('get_launcher_settings');
      if (savedSettings) {
        launcherSettings = { ...launcherSettings, ...savedSettings };
      }
    } catch (_) { }

    try {
      const walls = await tauriInvoke('get_available_wallpapers');
      if (Array.isArray(walls) && walls.length > 0) {
        availableWallpapers = walls;
      }
    } catch (_) { }

    applySettingsToUi();

    try {
      const games = await tauriInvoke('get_game_library');
      if (Array.isArray(games)) {
        allGames = games;
        applyFiltersAndSort();
      }
    } catch (e) {
      console.warn('get_game_library fallback:', e);
    }

    try {
      const media = await tauriInvoke('get_local_media_gallery');
      if (Array.isArray(media)) localMediaItems = media;
    } catch (_) { }

    try {
      const pairing = await tauriInvoke('get_mobile_pairing_info');
      if (pairing) renderPairingInfo(pairing);
    } catch (_) { }

    try {
      const telemetry = await tauriInvoke('get_system_telemetry');
      if (telemetry) renderTelemetry(telemetry);
    } catch (_) { }

    renderTileStrip();
    loadAndRenderSystemSpecs();

    if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) {
      window.__TAURI__.event.listen('mobile-controller-input', (ev) => {
        if (!document.hasFocus() || activeGameRunning) return;
        const ctrl = typeof ev.payload === 'string' ? ev.payload : ev.payload?.control;
        if (ctrl) {
          handleControllerCommand(ctrl);
        }
      });

      window.__TAURI__.event.listen('mobile-pairing-updated', (ev) => {
        if (ev.payload) renderPairingInfo(ev.payload);
      });

      window.__TAURI__.event.listen('playcraft-overlay-toggle', () => {
        toggleGlobalOverlay();
      });

      window.__TAURI__.event.listen('playcraft-request-exit', () => {
        openExitConfirmation('Alt+F4 / Close Button');
      });

      window.__TAURI__.event.listen('game-session-ended', async (ev) => {
        activeGameRunning = false;
        const p = ev.payload || {};
        const restoredCount = (p.restored_processes || []).length;
        try {
          const refreshed = await tauriInvoke('get_game_library');
          if (Array.isArray(refreshed)) {
            allGames = refreshed;
            applyFiltersAndSort();
            renderTileStrip();
          }
        } catch (_) { }
        showToast(
          'Game Session Ended',
          `${p.title || 'Game'} exited. Playtime logged & restored ${restoredCount} processes.`
        );
      });
    }
  }

  function renderPairingInfo(pairing) {
    latestMobilePairingInfo = pairing;
    const qrBox = document.getElementById('qrCodeContainer');
    const pinVal = document.getElementById('pairingPinValue');
    const ipsTxt = document.getElementById('availableIpsText');
    const btTxt = document.getElementById('btSppStatusText');
    const urlTxt = document.getElementById('pairingUrlText');
    const wsTxt = document.getElementById('wsUrlText');
    const backendTxt = document.getElementById('controllerBackendText');
    const apkTxt = document.getElementById('mobileApkPathText');
    const inputsBadge = document.getElementById('totalInputsBadge');
    const clientsList = document.getElementById('connectedClientsList');

    const pin = pairing.pin_code || '4829';
    const ips = Array.isArray(pairing.available_ips) && pairing.available_ips.length
      ? pairing.available_ips.join('  |  ')
      : (pairing.ip_address || '127.0.0.1');
    const url = pairing.pairing_url || 'playcraft://pair';
    const ws = pairing.ws_url || 'ws://127.0.0.1:19550/ws';
    const backend = pairing.virtual_controller_backend || 'Win32 Hardware ScanCode Bridge';
    const totalInputs = pairing.total_inputs_processed ?? 0;
    const clients = pairing.connected_clients || [];

    if (qrBox && pairing.qr_svg) qrBox.innerHTML = pairing.qr_svg;
    if (pinVal) pinVal.textContent = pin;
    if (ipsTxt) ipsTxt.textContent = ips;
    if (btTxt) {
      btTxt.textContent = pairing.bluetooth_spp_active
        ? `Ready (${pairing.bluetooth_pc_name || 'PC Radio'})`
        : `Offline (Wi-Fi / Hotspot / USB ready)`;
    }
    if (urlTxt) urlTxt.textContent = url;
    if (wsTxt) wsTxt.textContent = ws;
    if (backendTxt) {
      backendTxt.textContent = pairing.vigem_active ? `${backend} (Active)` : backend;
    }
    if (apkTxt) {
      apkTxt.textContent = pairing.apk_path || 'Download PlayCraft-Controller.apk from GitHub Releases';
    }
    if (inputsBadge) inputsBadge.textContent = `${totalInputs} Inputs Processed`;

    if (clientsList) {
      clientsList.textContent = '';
      if (clients.length > 0) {
        clients.forEach((c) => {
          const row = document.createElement('div');
          const rtt = typeof c.latency_ms === 'number' && c.latency_ms > 0 ? ` • ${c.latency_ms}ms` : '';
          row.textContent =
            typeof c === 'string'
              ? `Paired: ${c}`
              : `Paired: ${c.remote_addr || c.id || 'Phone'} (${c.user_agent || 'App'})${rtt} — ${c.last_input || 'Connected'}`;
          clientsList.appendChild(row);
        });
      } else {
        const empty = document.createElement('div');
        empty.className = 'empty-client';
        empty.textContent = 'Open PlayCraft Controller Android App & tap Auto-Find PC or Scan QR...';
        clientsList.appendChild(empty);
      }
    }
  }

  function renderTelemetry(telemetry) {
    const ramText = document.getElementById('ramTelemetryText');
    const ovCpu = document.getElementById('ovCpu');
    const ovRam = document.getElementById('ovRam');
    const ovActive = document.getElementById('ovActiveGame');
    const ovCtrl = document.getElementById('ovControllers');

    const usedGb = ((telemetry.ram_used_mb || 0) / 1024).toFixed(1);
    const totalGb = ((telemetry.ram_total_mb || 1) / 1024).toFixed(1);

    if (ramText) ramText.textContent = `RAM ${usedGb} / ${totalGb} GB`;
    if (ovCpu) ovCpu.textContent = `${telemetry.cpu_usage_percent ?? 0.0}%`;
    if (ovRam) ovRam.textContent = `${usedGb} / ${totalGb} GB`;
    if (ovActive) {
      ovActive.textContent = telemetry.active_game_title || 'Idle';
    }
    activeGameRunning = Boolean(telemetry.active_game_title);
    if (ovCtrl) {
      const mCount = telemetry.mobile_clients_connected || 0;
      const xCount = telemetry.xinput_controllers_connected || 0;
      ovCtrl.textContent = mCount + xCount > 0 ? `${mCount} Mobile + ${xCount} XInput` : 'Ready';
    }
  }

  function requestUserConfirmation({
    title = 'Are You Sure?',
    subtitle = 'Confirm Action',
    message = 'Do you want to proceed?',
    warning = '',
    proceedLabel = 'Confirm',
    cancelLabel = 'Cancel (Esc)',
    secondaryLabel = '',
    danger = false,
  }) {
    const dlg = document.getElementById('confirmActionDialog');
    if (!dlg) return Promise.resolve('proceed');

    if (activeConfirmResolver) {
      activeConfirmResolver('cancel');
      activeConfirmResolver = null;
    }

    const titleEl = document.getElementById('confirmDialogTitle');
    const subEl = document.getElementById('confirmDialogSubtitle');
    const msgEl = document.getElementById('confirmDialogMessage');
    const warnEl = document.getElementById('confirmDialogWarning');
    const cancelBtn = document.getElementById('confirmCancelBtn');
    const secBtn = document.getElementById('confirmSecondaryBtn');
    const proceedBtn = document.getElementById('confirmProceedBtn');

    if (titleEl) titleEl.textContent = title;
    if (subEl) subEl.textContent = subtitle;
    if (msgEl) msgEl.textContent = message;
    if (warnEl) {
      if (warning) {
        warnEl.textContent = warning;
        warnEl.classList.remove('hidden');
      } else {
        warnEl.textContent = '';
        warnEl.classList.add('hidden');
      }
    }
    if (cancelBtn) cancelBtn.textContent = cancelLabel;
    if (secBtn) {
      if (secondaryLabel) {
        secBtn.textContent = secondaryLabel;
        secBtn.classList.remove('hidden');
      } else {
        secBtn.classList.add('hidden');
      }
    }
    if (proceedBtn) {
      proceedBtn.textContent = proceedLabel;
      proceedBtn.classList.toggle('danger-solid-btn', Boolean(danger));
    }

    sound.playNavTick();
    if (!dlg.open) dlg.showModal();
    setTimeout(() => proceedBtn?.focus(), 40);

    return new Promise((resolve) => {
      activeConfirmResolver = (choice) => {
        activeConfirmResolver = null;
        if (dlg.open) dlg.close();
        resolve(choice);
      };
    });
  }

  async function openExitConfirmation(sourceLabel = 'Close Button') {
    if (exitConfirmOpen) return;
    exitConfirmOpen = true;
    try {
      const mobileCount = latestMobilePairingInfo?.connected_clients?.length || 0;
      const warnings = [];
      if (activeGameRunning) {
        warnings.push('• A game session is currently active — closing PlayCraft will stop playtime tracking.');
      }
      if (mobileCount > 0) {
        warnings.push(`• ${mobileCount} Mobile Controller(s) currently connected — exiting will disconnect your phone (choose "Minimize to Tray" if you want to keep driving!).`);
      }
      warnings.push('• Virtual Xbox 360 driver will be unplugged, paused background apps restored, and mobile bridge stopped.');

      const choice = await requestUserConfirmation({
        title: 'Exit PlayCraft Launcher?',
        subtitle: `Triggered via ${sourceLabel}`,
        message: 'Are you sure you want to completely close PlayCraft?',
        warning: warnings.join('\n'),
        proceedLabel: 'Yes, Exit PlayCraft',
        secondaryLabel: 'Minimize to Tray',
        cancelLabel: 'Stay Open (Esc)',
        danger: true,
      });

      if (choice === 'proceed') {
        try { sound.playLaunchSwell(); } catch (_) { }
        showToast('Exiting PlayCraft', 'Unplugging virtual controller & restoring background processes...');
        try {
          await tauriInvoke('quit_application');
        } catch (_) {
          window.close();
        }
      } else if (choice === 'secondary') {
        try {
          await tauriInvoke('minimize_launcher_to_tray');
          showToast('Minimized to System Tray', 'PlayCraft & Mobile Controller Bridge remain active in tray.');
        } catch (_) {}
      } else {
        showToast('Exit Cancelled', 'PlayCraft remains running.');
      }
    } finally {
      exitConfirmOpen = false;
    }
  }

  async function launchSelectedGame() {
    const list = getActiveList();
    if (!list.length) {
      openModal('scannerDialog');
      return;
    }
    const game = list[selectedIndex];
    if (!game) return;

    if (game.id === 'media-explore-hub') {
      await triggerOpenMedal();
      return;
    }

    const optNote = game.deep_optimize_before_launch
      ? '• Pre-Launch Deep Optimization is ON (Standby RAM purge & bloat trimming will run).'
      : '• Standard launch mode (Deep Optimization disabled for this game).';
    const drmNote = game.drmNorm?.enabled
      ? `• Custom Launch Flags Active (${game.drmNorm.custom_launch_args || 'Direct Binary'}).`
      : '';
    const stayNote = '• PlayCraft will remain open in the background so your Mobile Controller keeps working.';

    const choice = await requestUserConfirmation({
      title: `Launch ${game.title}?`,
      subtitle: game.genre || 'Game Launch Confirmation',
      message: `Are you sure you want to open '${game.title}'?`,
      warning: [optNote, drmNote, stayNote].filter(Boolean).join('\n'),
      proceedLabel: 'Yes, Launch Game',
      cancelLabel: 'Cancel (Esc)',
      danger: false,
    });

    if (choice !== 'proceed') {
      showToast('Launch Cancelled', `Cancelled starting '${game.title}'.`);
      return;
    }

    sound.playLaunchSwell();
    showToast('Launching Game', `Starting '${game.title}'...`);

    try {
      const res = await tauriInvoke('launch_game', { gameId: game.id });
      activeGameRunning = true;
      const pidInfo = res.pid ? ` (PID ${res.pid})` : '';
      const optSummary = res.optimization_report ? ` • ${res.optimization_report.summary}` : '';
      showToast(`Launched: ${res.title || game.title}`, `Active${pidInfo}${optSummary}`);
    } catch (err) {
      showToast('Launch Error', String(err));
    }
  }

  async function executeDeepOptimizeNow() {
    const gameMode = document.getElementById('optGameMode')?.checked ?? true;
    const purgeRam = document.getElementById('optPurgeRam')?.checked ?? true;
    const killBloat = document.getElementById('optKillBloat')?.checked ?? true;
    const autoRestore = document.getElementById('optAutoRestore')?.checked ?? true;
    const bloatRaw = document.getElementById('optBloatList')?.value || '';
    const bloatList = bloatRaw
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    const choice = await requestUserConfirmation({
      title: 'Run Deep System Optimization?',
      subtitle: 'Memory Purge & Background Process Trim',
      message: 'Are you sure you want to optimize system RAM and pause listed background apps right now?',
      warning: killBloat
        ? `Warning: Enabled bloat targets (${bloatList.slice(0, 5).join(', ') || 'configured apps'}) will be temporarily closed to free RAM and restored when your game ends.`
        : 'Standby RAM working sets will be trimmed without terminating background apps.',
      proceedLabel: 'Run Optimization',
      cancelLabel: 'Cancel (Esc)',
    });
    if (choice !== 'proceed') {
      showToast('Optimization Cancelled', 'No background processes were modified.');
      return;
    }

    try {
      await tauriInvoke('save_optimizer_config', {
        config: {
          enable_windows_game_mode: gameMode,
          purge_standby_ram: purgeRam,
          auto_kill_bloat_processes: killBloat,
          auto_restore_after_exit: autoRestore,
          elevate_game_priority: true,
          target_bloat_names: bloatList,
        },
      });
      const report = await tauriInvoke('run_deep_optimizations');
      const box = document.getElementById('optimizerReportBox');
      if (box && report.summary) box.textContent = report.summary;
      showToast(
        'Deep Optimization Complete',
        `Trimmed ${report.processes_trimmed} working sets • Reclaimed ${report.ram_freed_mb} MB RAM.`
      );
    } catch (err) {
      showToast('Optimizer Notice', String(err));
    }
  }

  async function restoreProcessesNow() {
    try {
      const restored = await tauriInvoke('restore_background_processes');
      const msg =
        Array.isArray(restored) && restored.length > 0
          ? `Restored ${restored.length} background processes: ${restored.join(', ')}`
          : 'All background processes are already active.';
      const box = document.getElementById('optimizerReportBox');
      if (box) box.textContent = msg;
      showToast('Process Auto-Restore', msg);
    } catch (err) {
      showToast('Restore Notice', String(err));
    }
  }

  function syncDrmModalWithGame(game) {
    if (!game) return;
    const titleEl = document.getElementById('drmTargetTitle');
    if (titleEl) titleEl.textContent = game.title;

    const cfg = game.drmNorm || {};
    const setCheck = (id, val) => {
      const el = document.getElementById(id);
      if (el) el.checked = Boolean(val);
    };
    setCheck('drmEnable', cfg.enabled);
    setCheck('drmDirectShipping', cfg.resolve_shipping_binary);
    setCheck('drmSteamStub', cfg.bypass_steam_stub);
    setCheck('drmEpicPortal', cfg.bypass_epic_portal);
    setCheck('drmOfflineEnv', cfg.force_offline_env);
    setCheck('drmHighPriority', cfg.high_priority_process);

    const argsEl = document.getElementById('drmCustomArgs');
    if (argsEl) argsEl.value = cfg.custom_launch_args || '';
  }

  async function refreshMonitorDropdown() {
    const monEl = document.getElementById('settingTargetMonitor');
    if (!monEl) return;
    try {
      const monitors = await tauriInvoke('get_available_monitors');
      if (Array.isArray(monitors) && monitors.length > 0) {
        const current = launcherSettings.target_monitor || 'primary';
        monEl.textContent = '';
        const primaryOpt = document.createElement('option');
        primaryOpt.value = 'primary';
        primaryOpt.textContent = 'Primary Monitor (Default)';
        monEl.appendChild(primaryOpt);
        monitors.forEach((m) => {
          const opt = document.createElement('option');
          opt.value = m.name;
          opt.textContent = m.label || m.name;
          monEl.appendChild(opt);
        });
        monEl.value = current;
      }
    } catch (_) { }
  }

  function openModal(id) {
    const dlg = document.getElementById(id);
    if (!dlg) return;
    sound.playNavTick();
    if (id === 'settingsDialog') {
      refreshMonitorDropdown();
      loadAndRenderSystemSpecs();
      renderWallpaperGallery();
    }
    if (!dlg.open) dlg.showModal();
  }

  async function toggleGlobalOverlay() {
    const drawer = document.getElementById('overlayDrawer');
    if (!drawer) return;
    sound.playToastChime();
    drawer.classList.toggle('hidden');
    try {
      await tauriInvoke('toggle_global_overlay');
    } catch (_) { }
  }

  // --------------------------------------------------------------------------
  // --------------------------------------------------------------------------
  // 14. Unified Keyboard & Controller Spatial Navigation Engine
  // --------------------------------------------------------------------------
  let activeFocusZone = 'TILES'; // 'TOPBAR' | 'TILES' | 'HERO' | 'MODAL'
  let heroFocusIndex = 0;
  let topbarFocusIndex = 0;
  let modalFocusIndex = 0;

  function clearAllFocusRings() {
    document.querySelectorAll('.ctrl-focus').forEach((el) => el.classList.remove('ctrl-focus'));
  }

  function getFocusableTopBarElements() {
    return [
      document.querySelector('.nav-link[data-nav-tab="games"]'),
      document.querySelector('.nav-link[data-nav-tab="media"]'),
      document.querySelector('.nav-link[data-nav-tab="analytics"]'),
      document.getElementById('searchToggleBtn'),
      document.getElementById('addGameTopBtn'),
      document.getElementById('openMobileBtn'),
      document.getElementById('openOptimizerBtn'),
      document.getElementById('fullscreenToggleBtn'),
      document.getElementById('openSettingsBtn'),
      document.getElementById('exitLauncherTopBtn'),
    ].filter((el) => Boolean(el && el.offsetParent !== null));
  }

  function getFocusableHeroButtons() {
    return [
      document.getElementById('playGameBtn'),
      document.getElementById('openGameWorkspaceBtn'),
      document.getElementById('drmConfigBtn'),
      document.getElementById('openInstallFolderBtn'),
      document.getElementById('removeGameBtn'),
    ].filter((el) => Boolean(el && el.offsetParent !== null && !el.classList.contains('hidden')));
  }

  function cycleTopTabs(delta) {
    const tabs = ['games', 'media', 'analytics'];
    let idx = tabs.indexOf(currentTab);
    if (idx === -1) idx = 0;
    idx = (idx + delta + tabs.length) % tabs.length;
    switchMainTab(tabs[idx]);
    sound.playNavTick();
  }

  function handleSpatialNav(action) {
    const openDialog = document.querySelector('dialog.ps5-dialog[open]');
    if (openDialog) {
      activeFocusZone = 'MODAL';
      const focusables = Array.from(
        openDialog.querySelectorAll(
          'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex="0"]'
        )
      ).filter((el) => el.offsetParent !== null && !el.classList.contains('hidden'));

      if (focusables.length === 0) return;

      if (action === 'DOWN' || action === 'RIGHT') {
        modalFocusIndex = (modalFocusIndex + 1) % focusables.length;
        clearAllFocusRings();
        focusables[modalFocusIndex]?.focus();
        focusables[modalFocusIndex]?.classList.add('ctrl-focus');
        sound.playNavTick();
      } else if (action === 'UP' || action === 'LEFT') {
        modalFocusIndex = (modalFocusIndex - 1 + focusables.length) % focusables.length;
        clearAllFocusRings();
        focusables[modalFocusIndex]?.focus();
        focusables[modalFocusIndex]?.classList.add('ctrl-focus');
        sound.playNavTick();
      } else if (action === 'ACTIVATE') {
        try { sound.playLaunchSwell(); } catch (_) { }
        const activeBtn = document.activeElement;
        if (activeBtn && openDialog.contains(activeBtn) && activeBtn.tagName === 'BUTTON') {
          activeBtn.click();
        } else {
          focusables[modalFocusIndex]?.click();
        }
      } else if (action === 'BACK') {
        sound.playNavTick();
        closeTopLayer(openDialog);
        activeFocusZone = 'TILES';
        clearAllFocusRings();
      }
      return;
    }

    // Main screen spatial navigation
    if (action === 'BACK') {
      sound.playNavTick();
      closeTileContextMenu();
      let closedSomething = false;
      const wrap = document.getElementById('searchBarWrap');
      if (wrap && !wrap.classList.contains('hidden')) {
        wrap.classList.add('hidden');
        closedSomething = true;
      }
      const drawer = document.getElementById('overlayDrawer');
      if (drawer && !drawer.classList.contains('hidden')) {
        drawer.classList.add('hidden');
        closedSomething = true;
      }
      activeFocusZone = 'TILES';
      clearAllFocusRings();
      if (!closedSomething) {
        openExitConfirmation('Escape / Back Key');
      }
      return;
    }

    if (action === 'UP') {
      if (activeFocusZone === 'HERO') {
        activeFocusZone = 'TILES';
        clearAllFocusRings();
        sound.playNavTick();
      } else if (activeFocusZone === 'TILES') {
        activeFocusZone = 'TOPBAR';
        clearAllFocusRings();
        const topEls = getFocusableTopBarElements();
        topbarFocusIndex = Math.min(topbarFocusIndex, Math.max(0, topEls.length - 1));
        topEls[topbarFocusIndex]?.classList.add('ctrl-focus');
        topEls[topbarFocusIndex]?.focus();
        sound.playNavTick();
      }
    } else if (action === 'DOWN') {
      if (activeFocusZone === 'TOPBAR') {
        activeFocusZone = 'TILES';
        clearAllFocusRings();
        sound.playNavTick();
      } else if (activeFocusZone === 'TILES') {
        activeFocusZone = 'HERO';
        clearAllFocusRings();
        const heroBtns = getFocusableHeroButtons();
        heroFocusIndex = 0;
        heroBtns[heroFocusIndex]?.classList.add('ctrl-focus');
        heroBtns[heroFocusIndex]?.focus();
        sound.playNavTick();
      }
    } else if (action === 'LEFT') {
      if (activeFocusZone === 'TILES') {
        selectGameIndex(selectedIndex - 1, true);
      } else if (activeFocusZone === 'HERO') {
        const heroBtns = getFocusableHeroButtons();
        if (heroBtns.length > 0) {
          heroFocusIndex = (heroFocusIndex - 1 + heroBtns.length) % heroBtns.length;
          clearAllFocusRings();
          heroBtns[heroFocusIndex]?.classList.add('ctrl-focus');
          heroBtns[heroFocusIndex]?.focus();
          sound.playNavTick();
        }
      } else if (activeFocusZone === 'TOPBAR') {
        const topEls = getFocusableTopBarElements();
        if (topEls.length > 0) {
          topbarFocusIndex = (topbarFocusIndex - 1 + topEls.length) % topEls.length;
          clearAllFocusRings();
          topEls[topbarFocusIndex]?.classList.add('ctrl-focus');
          topEls[topbarFocusIndex]?.focus();
          sound.playNavTick();
        }
      }
    } else if (action === 'RIGHT') {
      if (activeFocusZone === 'TILES') {
        selectGameIndex(selectedIndex + 1, true);
      } else if (activeFocusZone === 'HERO') {
        const heroBtns = getFocusableHeroButtons();
        if (heroBtns.length > 0) {
          heroFocusIndex = (heroFocusIndex + 1) % heroBtns.length;
          clearAllFocusRings();
          heroBtns[heroFocusIndex]?.classList.add('ctrl-focus');
          heroBtns[heroFocusIndex]?.focus();
          sound.playNavTick();
        }
      } else if (activeFocusZone === 'TOPBAR') {
        const topEls = getFocusableTopBarElements();
        if (topEls.length > 0) {
          topbarFocusIndex = (topbarFocusIndex + 1) % topEls.length;
          clearAllFocusRings();
          topEls[topbarFocusIndex]?.classList.add('ctrl-focus');
          topEls[topbarFocusIndex]?.focus();
          sound.playNavTick();
        }
      }
    } else if (action === 'ACTIVATE') {
      if (activeFocusZone === 'TILES') {
        launchSelectedGame();
      } else if (activeFocusZone === 'HERO') {
        const heroBtns = getFocusableHeroButtons();
        heroBtns[heroFocusIndex]?.click();
      } else if (activeFocusZone === 'TOPBAR') {
        const topEls = getFocusableTopBarElements();
        topEls[topbarFocusIndex]?.click();
      }
    }
  }

  function handleControllerCommand(cmd) {
    const c = String(cmd).toUpperCase();
    if (c === 'LEFT' || c === 'L1') {
      if (c === 'L1') cycleTopTabs(-1);
      else handleSpatialNav('LEFT');
    } else if (c === 'RIGHT' || c === 'R1') {
      if (c === 'R1') cycleTopTabs(1);
      else handleSpatialNav('RIGHT');
    } else if (c === 'UP') {
      handleSpatialNav('UP');
    } else if (c === 'DOWN') {
      handleSpatialNav('DOWN');
    } else if (c === 'CROSS') {
      handleSpatialNav('ACTIVATE');
    } else if (c === 'CIRCLE') {
      handleSpatialNav('BACK');
    } else if (c === 'SQUARE') {
      executeDeepOptimizeNow();
    } else if (c === 'TRIANGLE' || c === 'TOUCHPAD') {
      document.getElementById('searchToggleBtn')?.click();
    } else if (c === 'PS_HOME') {
      toggleGlobalOverlay();
    }
  }

  // Native PC Gamepad Controller Poller (Xbox, DualSense, 8BitDo, Switch Pro)
  let lastGamepadActionTime = 0;
  function startGamepadPoller() {
    function loop(now) {
      if (!document.hasFocus() || activeGameRunning) {
        requestAnimationFrame(loop);
        return;
      }
      const gamepads = typeof navigator.getGamepads === 'function' ? navigator.getGamepads() : [];
      for (const gp of gamepads) {
        if (!gp || !gp.connected) continue;

        const bootOverlay = document.getElementById('bootIntroOverlay');
        if (bootOverlay && !bootOverlay.classList.contains('fade-out')) {
          const anyPressed = gp.buttons.some((b) => Boolean(b?.pressed));
          if (anyPressed) {
            lastGamepadActionTime = now + 300;
            if (typeof window.__playcraftDismissBoot === 'function') {
              window.__playcraftDismissBoot();
            }
            break;
          }
        }

        if (now - lastGamepadActionTime > 190) {
          const up = gp.buttons[12]?.pressed || gp.axes[1] < -0.55;
          const down = gp.buttons[13]?.pressed || gp.axes[1] > 0.55;
          const left = gp.buttons[14]?.pressed || gp.axes[0] < -0.55;
          const right = gp.buttons[15]?.pressed || gp.axes[0] > 0.55;

          const btnA = gp.buttons[0]?.pressed;
          const btnB = gp.buttons[1]?.pressed;
          const btnX = gp.buttons[2]?.pressed;
          const btnY = gp.buttons[3]?.pressed;
          const btnLB = gp.buttons[4]?.pressed;
          const btnRB = gp.buttons[5]?.pressed;
          const btnBack = gp.buttons[8]?.pressed;
          const btnStart = gp.buttons[9]?.pressed;
          const btnGuide = gp.buttons[16]?.pressed;

          if (up) {
            lastGamepadActionTime = now;
            handleSpatialNav('UP');
            break;
          } else if (down) {
            lastGamepadActionTime = now;
            handleSpatialNav('DOWN');
            break;
          } else if (left) {
            lastGamepadActionTime = now;
            handleSpatialNav('LEFT');
            break;
          } else if (right) {
            lastGamepadActionTime = now;
            handleSpatialNav('RIGHT');
            break;
          } else if (btnA) {
            lastGamepadActionTime = now + 90;
            handleSpatialNav('ACTIVATE');
            break;
          } else if (btnB) {
            lastGamepadActionTime = now + 90;
            handleSpatialNav('BACK');
            break;
          } else if (btnX) {
            lastGamepadActionTime = now + 120;
            executeDeepOptimizeNow();
            break;
          } else if (btnY) {
            lastGamepadActionTime = now + 120;
            document.getElementById('searchToggleBtn')?.click();
            break;
          } else if (btnLB) {
            lastGamepadActionTime = now + 120;
            cycleTopTabs(-1);
            break;
          } else if (btnRB) {
            lastGamepadActionTime = now + 120;
            cycleTopTabs(1);
            break;
          } else if (btnStart) {
            lastGamepadActionTime = now + 180;
            openModal('settingsDialog');
            break;
          } else if (btnBack) {
            lastGamepadActionTime = now + 180;
            renderLibraryFullGrid();
            openModal('libraryGridDialog');
            break;
          } else if (btnGuide) {
            lastGamepadActionTime = now + 180;
            toggleGlobalOverlay();
            break;
          }
        }
      }
      requestAnimationFrame(loop);
    }
    requestAnimationFrame(loop);
  }

  function startTelemetryPoller() {
    setInterval(async () => {
      try {
        const telemetry = await tauriInvoke('get_system_telemetry');
        if (telemetry) renderTelemetry(telemetry);
        const pairing = await tauriInvoke('get_mobile_pairing_info');
        if (pairing) renderPairingInfo(pairing);
      } catch (_) { }
    }, 1500);
  }

  function startFrameRateCounter() {
    const fpsEl = document.getElementById('fpsCounter');
    if (!fpsEl) return;
    let frameCount = 0;
    let lastFpsTime = performance.now();
    function tick(now) {
      frameCount++;
      if (now - lastFpsTime >= 500) {
        const fps = Math.min(240, Math.max(60, Math.round((frameCount * 1000) / (now - lastFpsTime))));
        fpsEl.textContent = `${fps} FPS`;
        frameCount = 0;
        lastFpsTime = now;
      }
      requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  }

  // --------------------------------------------------------------------------
  // 15. Wire Up UI Buttons, Settings Tabs, Workspace & File Pickers
  // --------------------------------------------------------------------------
  function wireUiInteractions() {
    const updateClock = () => {
      const clock = document.getElementById('clockDisplay');
      if (!clock) return;
      const now = new Date();
      clock.textContent = now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    };
    updateClock();
    setInterval(updateClock, 15000);

    // Top bar tabs (Games / Media)
    document.querySelectorAll('.ps5-tab[data-tab]').forEach((tabBtn) => {
      tabBtn.addEventListener('click', async () => {
        document.querySelectorAll('.ps5-tab[data-tab]').forEach((b) => {
          b.classList.remove('active');
          b.setAttribute('aria-selected', 'false');
        });
        tabBtn.classList.add('active');
        tabBtn.setAttribute('aria-selected', 'true');
        currentTab = tabBtn.getAttribute('data-tab') || 'games';
        selectedIndex = 0;
        if (currentTab === 'media') {
          try {
            const media = await tauriInvoke('get_local_media_gallery');
            if (Array.isArray(media)) localMediaItems = media;
          } catch (_) { }
        }
        sound.playNavTick();
        renderTileStrip();
      });
    });

    document.getElementById('openAnalyticsTabBtn')?.addEventListener('click', openAnalyticsModal);
    document.getElementById('openMedalTopBtn')?.addEventListener('click', triggerOpenMedal);
    document.getElementById('ovOpenMedalBtn')?.addEventListener('click', triggerOpenMedal);

    // Settings Modal Navigation Tabs
    document.querySelectorAll('.settings-nav-btn[data-settings-tab]').forEach((btn) => {
      btn.addEventListener('click', () => {
        document.querySelectorAll('.settings-nav-btn[data-settings-tab]').forEach((b) =>
          b.classList.remove('active')
        );
        document.querySelectorAll('.settings-pane').forEach((p) => p.classList.add('hidden'));
        btn.classList.add('active');
        const targetId = btn.getAttribute('data-settings-tab');
        if (targetId) {
          document.getElementById(targetId)?.classList.remove('hidden');
          if (targetId === 'tab-specs') loadAndRenderSystemSpecs();
        }
        sound.playNavTick();
      });
    });

    // Settings Checkboxes
    document.getElementById('settingLockWallpaper')?.addEventListener('change', async (e) => {
      launcherSettings.wallpaper_mode = e.target.checked ? 'wallpaper_priority' : 'game_hero_priority';
      applySettingsToUi();
      await persistSettings();
    });

    const bindSettingCheckbox = (elId, key) => {
      const el = document.getElementById(elId);
      if (!el) return;
      el.addEventListener('change', async () => {
        launcherSettings[key] = Boolean(el.checked);
        applySettingsToUi();
        await persistSettings();
      });
    };

    // Legacy checkbox removed in favor of #settingWindowMode select (fullscreen | borderless | windowed)
    bindSettingCheckbox('settingShowFps', 'show_fps_counter');
    bindSettingCheckbox('settingUiSounds', 'enable_ui_sounds');
    bindSettingCheckbox('settingBootVideo', 'enable_boot_video');

    document.getElementById('refreshSpecsBtn')?.addEventListener('click', () => {
      loadAndRenderSystemSpecs();
      showToast('Hardware Specs Refreshed', 'Queried live CPU, GPU, RAM, and Disk telemetry.');
    });

    const modeLabel = (m) => {
      if (m === 'borderless') return 'Windowed Fullscreen (Borderless)';
      if (m === 'windowed') return 'Normal Window (Resizable)';
      return 'Exclusive Fullscreen';
    };

    const cycleDisplayMode = async () => {
      try {
        const nextMode = await tauriInvoke('toggle_fullscreen_window');
        launcherSettings.window_mode = nextMode;
        launcherSettings.start_fullscreen = nextMode !== 'windowed';
        applySettingsToUi();
        showToast('Display Mode', `Switched to ${modeLabel(nextMode)}`);
      } catch (_) { }
    };

    document.getElementById('settingWindowMode')?.addEventListener('change', async (e) => {
      launcherSettings.window_mode = e.target.value;
      launcherSettings.start_fullscreen = e.target.value !== 'windowed';
      await persistSettings();
      showToast('Display Mode Updated', `Switched to ${modeLabel(e.target.value)}`);
    });

    document.getElementById('settingTargetMonitor')?.addEventListener('change', async (e) => {
      launcherSettings.target_monitor = e.target.value;
      await persistSettings();
      showToast('Monitor Updated', `Launcher moved to: ${e.target.selectedOptions[0]?.text || e.target.value}`);
    });

    document.getElementById('toggleFullscreenFromSettingsBtn')?.addEventListener('click', cycleDisplayMode);
    document.getElementById('fullscreenToggleBtn')?.addEventListener('click', cycleDisplayMode);

    document.getElementById('openSoundSettingsFromTabBtn')?.addEventListener('click', async () => {
      try {
        await tauriInvoke('open_windows_sound_settings');
      } catch (_) { }
    });

    // Modal Open Triggers
    document.getElementById('openSettingsBtn')?.addEventListener('click', () => openModal('settingsDialog'));
    document.getElementById('openMobileBtn')?.addEventListener('click', () => openModal('mobileDialog'));
    document.getElementById('openOptimizerBtn')?.addEventListener('click', () => openModal('optimizerDialog'));
    document.getElementById('drmConfigBtn')?.addEventListener('click', () => openModal('optimizerDialog'));
    document.getElementById('openScannerBtn')?.addEventListener('click', () => openModal('scannerDialog'));
    document.getElementById('openGameWorkspaceBtn')?.addEventListener('click', openGameWorkspaceModal);
    document.getElementById('logoAboutTrigger')?.addEventListener('click', () => {
      openModal('settingsDialog');
      document.querySelector('.settings-nav-btn[data-settings-tab="tab-about"]')?.click();
    });

    document.getElementById('playGameBtn')?.addEventListener('click', launchSelectedGame);

    // Open Install Folder in Explorer
    document.getElementById('openInstallFolderBtn')?.addEventListener('click', async () => {
      const list = getActiveList();
      const game = list[selectedIndex];
      if (!game) return;
      const targetPath = game.working_dir || game.exe_path;
      try {
        await tauriInvoke('open_path_in_explorer', { targetPath });
      } catch (e) {
        showToast('Explorer Notice', String(e));
      }
    });

    // Per-Game Workspace Modal Actions
    document.getElementById('wsPickSaveFolderBtn')?.addEventListener('click', async () => {
      try {
        const folder = await tauriInvoke('pick_scan_directory');
        if (folder) document.getElementById('wsSaveFolderPath').value = folder;
      } catch (_) { }
    });

    document.getElementById('wsOpenSaveFolderBtn')?.addEventListener('click', async () => {
      const p = document.getElementById('wsSaveFolderPath')?.value.trim();
      if (!p) return showToast('Save Folder Empty', 'Select a Save Folder path first.');
      try {
        await tauriInvoke('open_path_in_explorer', { targetPath: p });
      } catch (e) {
        showToast('Explorer Notice', String(e));
      }
    });

    document.getElementById('wsPickModFolderBtn')?.addEventListener('click', async () => {
      try {
        const folder = await tauriInvoke('pick_scan_directory');
        if (folder) document.getElementById('wsModFolderPath').value = folder;
      } catch (_) { }
    });

    document.getElementById('wsPickCoverImgBtn')?.addEventListener('click', async () => {
      try {
        const dataUrl = await tauriInvoke('pick_local_image_file');
        if (dataUrl) {
          const inp = document.getElementById('wsCoverUrlInput');
          if (inp) inp.value = dataUrl;
          await saveCurrentWorkspaceConfig(true);
        }
      } catch (_) { }
    });

    document.getElementById('wsPickHeroImgBtn')?.addEventListener('click', async () => {
      try {
        const dataUrl = await tauriInvoke('pick_local_image_file');
        if (dataUrl) {
          const inp = document.getElementById('wsHeroUrlInput');
          if (inp) inp.value = dataUrl;
          await saveCurrentWorkspaceConfig(true);
        }
      } catch (_) { }
    });

    document.getElementById('wsOpenModFolderBtn')?.addEventListener('click', async () => {
      const p = document.getElementById('wsModFolderPath')?.value.trim();
      if (!p) return showToast('Folder Empty', 'Select a Mod / Game Folder path first.');
      try {
        await tauriInvoke('open_path_in_explorer', { targetPath: p });
      } catch (e) {
        showToast('Explorer Notice', String(e));
      }
    });

    document.getElementById('wsOpenAudioMixerBtn')?.addEventListener('click', async () => {
      try {
        await tauriInvoke('open_windows_sound_settings');
      } catch (_) { }
    });

    document.getElementById('wsCreateBackupNowBtn')?.addEventListener('click', async () => {
      const list = getActiveList();
      const game = list[selectedIndex];
      if (!game) return;
      // Save current save folder path first
      await saveCurrentWorkspaceConfig(false);
      try {
        const snap = await tauriInvoke('create_save_backup', { gameId: game.id });
        await refreshGameSaveBackupsList(game.id);
        showToast('Save Snapshot Created', `${snap.timestamp} (${snap.file_count} files, ${snap.size_mb} MB)`);
      } catch (e) {
        showToast('Backup Notice', String(e));
      }
    });

    async function saveCurrentWorkspaceConfig(showConfirmation = true) {
      const list = getActiveList();
      const game = list[selectedIndex];
      if (!game) return;

      const modeVal = document.getElementById('wsDisplayModeSelect')?.value || '';
      let targetRes = null;
      let targetHz = null;
      if (modeVal.includes('@')) {
        const [r, h] = modeVal.split('@');
        targetRes = r;
        targetHz = parseInt(h, 10) || null;
      }

      const update = {
        game_id: game.id,
        notes: document.getElementById('wsGameNotes')?.value || '',
        save_folder_path: document.getElementById('wsSaveFolderPath')?.value.trim() || '',
        auto_backup_saves: Boolean(document.getElementById('wsAutoBackupSaves')?.checked),
        mod_folder_path: document.getElementById('wsModFolderPath')?.value.trim() || '',
        active_launch_profile: document.getElementById('wsLaunchProfileSelect')?.value || 'Default',
        target_refresh_hz: targetHz,
        target_resolution: targetRes,
        cover_url: document.getElementById('wsCoverUrlInput')?.value.trim() || null,
        hero_url: document.getElementById('wsHeroUrlInput')?.value.trim() || null,
      };

      try {
        await tauriInvoke('update_game_workspace_config', { update });
        const refreshed = await tauriInvoke('get_game_library');
        if (Array.isArray(refreshed)) {
          allGames = refreshed;
          applyFiltersAndSort();
          renderTileStrip();
        }
        if (showConfirmation) {
          showToast('Workspace Saved', `Updated saves, refresh rate & notes for '${game.title}'.`);
        }
      } catch (e) {
        if (showConfirmation) showToast('Save Error', String(e));
      }
    }

    document.getElementById('wsSaveConfigBtn')?.addEventListener('click', () => saveCurrentWorkspaceConfig(true));

    // Favorite / Pin Game Button
    document.getElementById('favoriteGameBtn')?.addEventListener('click', toggleFavoriteSelectedGame);

    // Desktop Right-Click Context Menu Actions
    document.addEventListener('click', (ev) => {
      const menu = document.getElementById('tileContextMenu');
      if (menu && !menu.classList.contains('hidden') && !menu.contains(ev.target)) {
        closeTileContextMenu();
      }
    });

    document.querySelectorAll('#tileContextMenu [data-ctx-action]').forEach((btn) => {
      btn.addEventListener('click', () => {
        const action = btn.getAttribute('data-ctx-action');
        closeTileContextMenu();
        switch (action) {
          case 'play':
            launchSelectedGame();
            break;
          case 'favorite':
            toggleFavoriteSelectedGame();
            break;
          case 'workspace':
            openGameWorkspaceModal();
            break;
          case 'drm':
            openModal('optimizerDialog');
            break;
          case 'folder':
            document.getElementById('openInstallFolderBtn')?.click();
            break;
          case 'remove':
            document.getElementById('removeGameBtn')?.click();
            break;
          default:
            break;
        }
      });
    });

    // Remove Game Button (with clear confirmation warning)
    document.getElementById('removeGameBtn')?.addEventListener('click', async () => {
      const list = getActiveList();
      const game = list[selectedIndex];
      if (!game || game.id === 'media-explore-hub') return;

      const choice = await requestUserConfirmation({
        title: `Remove '${game.title}'?`,
        subtitle: 'Library Removal Warning',
        message: `Are you sure you want to remove '${game.title}' from your PlayCraft library?`,
        warning: 'Only removes the shortcut from PlayCraft — your actual game folder and files on disk will NOT be deleted.',
        proceedLabel: 'Yes, Remove',
        cancelLabel: 'Keep in Library',
        danger: true,
      });
      if (choice !== 'proceed') {
        showToast('Removal Cancelled', `'${game.title}' remains in your library.`);
        return;
      }

      try {
        const updated = await tauriInvoke('remove_game_from_library', { gameId: game.id });
        if (Array.isArray(updated)) {
          allGames = updated;
          applyFiltersAndSort();
          selectedIndex = 0;
          renderTileStrip();
          showToast('Removed from Library', `Removed '${game.title}' from PlayCraft.`);
        }
      } catch (err) {
        showToast('Remove Error', String(err));
      }
    });

    // USB Cable ADB Bridge Trigger inside Mobile Pairing Dialog
    document.getElementById('setupUsbBridgeBtn')?.addEventListener('click', async () => {
      showToast('USB Bridge', 'Configuring adb reverse tcp:19550 for wired phone connection...');
      try {
        const msg = await tauriInvoke('setup_usb_mobile_bridge');
        showToast('USB Bridge Ready', msg);
      } catch (err) {
        showToast('USB Bridge Warning', String(err));
      }
    });

    // Confirmation Dialog Button Handlers
    document.getElementById('confirmCancelBtn')?.addEventListener('click', () => activeConfirmResolver?.('cancel'));
    document.getElementById('confirmCloseXBtn')?.addEventListener('click', () => activeConfirmResolver?.('cancel'));
    document.getElementById('confirmSecondaryBtn')?.addEventListener('click', () => activeConfirmResolver?.('secondary'));
    document.getElementById('confirmProceedBtn')?.addEventListener('click', () => activeConfirmResolver?.('proceed'));
    document.getElementById('confirmActionDialog')?.addEventListener('cancel', (ev) => {
      ev.preventDefault();
      activeConfirmResolver?.('cancel');
    });
    document.getElementById('confirmActionDialog')?.addEventListener('close', () => {
      if (activeConfirmResolver) activeConfirmResolver('cancel');
    });

    // Overlay Controls
    document.getElementById('closeOverlayBtn')?.addEventListener('click', toggleGlobalOverlay);
    document.getElementById('ovPurgeRamBtn')?.addEventListener('click', executeDeepOptimizeNow);
    document.getElementById('ovOpenMobileBtn')?.addEventListener('click', () => {
      document.getElementById('overlayDrawer')?.classList.add('hidden');
      openModal('mobileDialog');
    });
    document.getElementById('ovRestoreBtn')?.addEventListener('click', restoreProcessesNow);

    // Search Filter, Store Filter Chips, Sort Dropdown & All Games Grid Drawer
    const searchWrap = document.getElementById('searchBarWrap');
    const searchInput = document.getElementById('searchInput');
    document.getElementById('searchToggleBtn')?.addEventListener('click', () => {
      searchWrap?.classList.toggle('hidden');
      if (!searchWrap?.classList.contains('hidden')) searchInput?.focus();
    });
    document.getElementById('clearSearchBtn')?.addEventListener('click', () => {
      if (searchInput) searchInput.value = '';
      currentStoreFilter = 'ALL';
      document.querySelectorAll('.filter-chip[data-store-filter]').forEach((c) => {
        c.classList.toggle('active', c.getAttribute('data-store-filter') === 'ALL');
      });
      applyFiltersAndSort();
      selectedIndex = 0;
      renderTileStrip();
    });
    searchInput?.addEventListener('input', () => {
      applyFiltersAndSort();
      selectedIndex = 0;
      renderTileStrip();
    });

    document.querySelectorAll('.filter-chip[data-store-filter]').forEach((chip) => {
      chip.addEventListener('click', () => {
        document.querySelectorAll('.filter-chip[data-store-filter]').forEach((c) =>
          c.classList.remove('active')
        );
        chip.classList.add('active');
        currentStoreFilter = chip.getAttribute('data-store-filter') || 'ALL';
        sound.playNavTick();
        applyFiltersAndSort();
        selectedIndex = 0;
        renderTileStrip();
      });
    });

    document.getElementById('librarySortSelect')?.addEventListener('change', (ev) => {
      currentSortOrder = ev.target.value || 'recent';
      sound.playNavTick();
      applyFiltersAndSort();
      selectedIndex = 0;
      renderTileStrip();
    });

    document.getElementById('toggleGridDrawerBtn')?.addEventListener('click', () => {
      renderLibraryFullGrid();
      openModal('libraryGridDialog');
    });
    document.getElementById('gridAddGameBtn')?.addEventListener('click', () => {
      closeTopLayer(document.getElementById('libraryGridDialog'));
      openModal('scannerDialog');
    });

    // Mobile Pairing Modal actions
    document.getElementById('regenPinBtn')?.addEventListener('click', async () => {
      try {
        const pairing = await tauriInvoke('regenerate_mobile_pin');
        renderPairingInfo(pairing);
        showToast('Security PIN Regenerated', `New Mobile Controller PIN: ${pairing.pin_code}`);
      } catch (e) {
        showToast('PIN Notice', String(e));
      }
    });

    document.querySelectorAll('.sim-btn[data-sim]').forEach((btn) => {
      btn.addEventListener('click', async () => {
        const code = btn.getAttribute('data-sim');
        try {
          await tauriInvoke('simulate_mobile_input', { control: code });
        } catch (_) {
          handleControllerCommand(code);
        }
      });
    });

    // Guided Add Game Modal: Native File Picker + Automatic Online DB Lookup
    document.getElementById('browseTargetFileBtn')?.addEventListener('click', async () => {
      try {
        const pickedResult = await tauriInvoke('pick_game_executable');
        if (!pickedResult) return;

        let resolvedPath = '';
        if (typeof pickedResult === 'string') {
          resolvedPath = pickedResult;
        } else if (pickedResult && typeof pickedResult === 'object') {
          resolvedPath =
            pickedResult.exe_path ||
            pickedResult.resolved_path ||
            pickedResult.path ||
            pickedResult.filePath ||
            (Array.isArray(pickedResult) ? pickedResult[0] : '');
          if (!resolvedPath && typeof pickedResult.toString === 'function') {
            const s = pickedResult.toString();
            if (s !== '[object Object]') resolvedPath = s;
          }
        }

        if (!resolvedPath || resolvedPath === '[object Object]') return;

        const exeInput = document.getElementById('customExePath');
        if (exeInput) exeInput.value = resolvedPath;

        const inspected =
          typeof pickedResult === 'object' && pickedResult?.clean_title_query
            ? pickedResult
            : await tauriInvoke('inspect_game_file', { filePath: resolvedPath });

        if (inspected) {
          const cleanQuery = inspected.clean_title_query || inspected.inferred_title || '';
          const titleEl = document.getElementById('customTitle');
          if (titleEl) titleEl.value = cleanQuery;

          const sizeBadge = document.getElementById('inspectedSizeBadge');
          if (sizeBadge) {
            sizeBadge.textContent = `Total Game Folder Size: ${inspected.size_on_disk_gb ?? 0.1} GB (${inspected.source_type || 'Executable'})`;
          }

          if (cleanQuery) {
            await searchAndAutoFillGameMetadata(cleanQuery, true);
          }
        }
      } catch (err) {
        showToast('File Picker Notice', String(err));
      }
    });

    // Local Cover & Hero Image File Pickers
    document.getElementById('pickCoverFileBtn')?.addEventListener('click', async () => {
      try {
        const dataUrl = await tauriInvoke('pick_local_image_file');
        if (dataUrl) {
          const coverEl = document.getElementById('customCoverUrl');
          const previewImg = document.getElementById('previewCoverImg');
          if (coverEl) coverEl.value = dataUrl;
          if (previewImg) previewImg.src = dataUrl;
          showToast('Cover Photo Selected', 'Applied local image as game cover art.');
        }
      } catch (e) {
        showToast('Image Picker Notice', String(e));
      }
    });

    document.getElementById('pickHeroFileBtn')?.addEventListener('click', async () => {
      try {
        const dataUrl = await tauriInvoke('pick_local_image_file');
        if (dataUrl) {
          const heroEl = document.getElementById('customHeroUrl');
          if (heroEl) heroEl.value = dataUrl;
          showToast('Hero Wallpaper Selected', 'Applied local image as game hero banner.');
        }
      } catch (e) {
        showToast('Image Picker Notice', String(e));
      }
    });

    document.getElementById('searchOnlineDbBtn')?.addEventListener('click', () => {
      const q = document.getElementById('onlineDbSearchInput')?.value.trim();
      if (q) searchAndAutoFillGameMetadata(q, true);
    });

    document.getElementById('onlineDbSearchInput')?.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const q = document.getElementById('onlineDbSearchInput')?.value.trim();
        if (q) searchAndAutoFillGameMetadata(q, true);
      }
    });

    document.getElementById('customCoverUrl')?.addEventListener('input', (e) => {
      const url = e.target.value.trim();
      const previewImg = document.getElementById('previewCoverImg');
      if (previewImg && url) previewImg.src = url;
    });

    // Native Folder Picker for Bulk Directory Scan
    document.getElementById('browseFolderBtn')?.addEventListener('click', async () => {
      try {
        const folder = await tauriInvoke('pick_scan_directory');
        if (folder) {
          const inp = document.getElementById('scanDirInput');
          if (inp) inp.value = folder;
        }
      } catch (err) {
        showToast('Folder Picker Notice', String(err));
      }
    });

    document.getElementById('runDirScanBtn')?.addEventListener('click', async () => {
      const pathVal = document.getElementById('scanDirInput')?.value.trim();
      if (!pathVal) {
        showToast('Select a Folder', 'Click "Pick Folder to Scan..." first.');
        return;
      }
      const statusBox = document.getElementById('scanStatusMsg');
      if (statusBox) statusBox.textContent = `Scanning '${pathVal}' recursively for .exe, .bat, .lnk...`;
      try {
        const updated = await tauriInvoke('scan_directory', { dirPath: pathVal });
        if (Array.isArray(updated)) {
          allGames = updated;
          applyFiltersAndSort();
          renderTileStrip();
        }
        const msg = `Scan complete for '${pathVal}' — Library now has ${allGames.length} games.`;
        if (statusBox) statusBox.textContent = msg;
        showToast('Directory Scan Complete', msg);
      } catch (err) {
        if (statusBox) statusBox.textContent = String(err);
        showToast('Scan Notice', String(err));
      }
    });

    document.getElementById('scanSteamBtn')?.addEventListener('click', async () => {
      const statusBox = document.getElementById('scanStatusMsg');
      if (statusBox) statusBox.textContent = 'Scanning Steam, Epic Games, GOG Galaxy & Ubisoft Connect manifests...';
      try {
        const updated = await tauriInvoke('rescan_steam_library');
        if (Array.isArray(updated)) {
          allGames = updated;
          applyFiltersAndSort();
          renderTileStrip();
        }
        const msg = `Multi-Store Auto-Sync complete — ${allGames.length} installed games in PlayCraft Library.`;
        if (statusBox) statusBox.textContent = msg;
        showToast('Multi-Store Library Synced', msg);
      } catch (err) {
        if (statusBox) statusBox.textContent = String(err);
      }
    });

    document.getElementById('addCustomGameForm')?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const title = document.getElementById('customTitle')?.value.trim();
      const exePath = document.getElementById('customExePath')?.value.trim();
      if (!title || !exePath) {
        showToast('Missing Game File', 'Please select a game executable (.exe, .bat, .lnk) first.');
        return;
      }

      const coverUrl = document.getElementById('customCoverUrl')?.value.trim() || '';
      const heroUrl = document.getElementById('customHeroUrl')?.value.trim() || coverUrl;
      const description = document.getElementById('customDescription')?.value.trim() || '';

      const req = {
        title,
        subtitle: null,
        description: description || null,
        genre: document.getElementById('customGenre')?.value.trim() || 'PC Game',
        exe_path: exePath,
        launch_args: document.getElementById('customArgs')?.value.trim() || '',
        cover_url: coverUrl || null,
        hero_url: heroUrl || null,
        steam_app_id: selectedOnlineSteamAppId,
        deep_optimize_before_launch: Boolean(document.getElementById('customDeepOpt')?.checked),
        drm_bypass: null,
      };

      try {
        const updated = await tauriInvoke('add_custom_game', { req });
        if (Array.isArray(updated)) {
          allGames = updated;
          applyFiltersAndSort();
          selectedIndex = 0;
          renderTileStrip();
        }
        selectedOnlineSteamAppId = null;
        showToast('Game Added to PlayCraft', `Added '${title}' with real disk telemetry & metadata.`);
        closeTopLayer(document.getElementById('scannerDialog'));
      } catch (err) {
        showToast('Add Game Error', String(err));
      }
    });

    document.getElementById('execDeepOptBtn')?.addEventListener('click', executeDeepOptimizeNow);
    document.getElementById('restoreProcsBtn')?.addEventListener('click', restoreProcessesNow);

    document.getElementById('saveGameDrmBtn')?.addEventListener('click', async () => {
      const list = getActiveList();
      const game = list[selectedIndex];
      if (!game) return;

      const customArgs = document.getElementById('drmCustomArgs')?.value.trim() || '';
      const drmBypass = {
        enabled: Boolean(document.getElementById('drmEnable')?.checked),
        force_direct_shipping_exe: Boolean(document.getElementById('drmDirectShipping')?.checked),
        bypass_steam_stub: Boolean(document.getElementById('drmSteamStub')?.checked),
        bypass_epic_portal: Boolean(document.getElementById('drmEpicPortal')?.checked),
        skip_intro_videos: true,
        offline_mode_env: Boolean(document.getElementById('drmOfflineEnv')?.checked),
        high_priority_process: Boolean(document.getElementById('drmHighPriority')?.checked),
        custom_args: customArgs,
      };

      try {
        await tauriInvoke('update_game_drm_config', {
          gameId: game.id,
          drmBypass,
          deepOptimize: true,
          launchArgs: customArgs,
        });
        const refreshed = await tauriInvoke('get_game_library');
        if (Array.isArray(refreshed)) {
          allGames = refreshed;
          applyFiltersAndSort();
        }
        showToast('Launch Flags Saved', `Updated per-game launch profile for '${game.title}'.`);
      } catch (err) {
        showToast('Save Notice', String(err));
      }
    });

    // Window & Background Behavior Settings Listeners
    document.getElementById('settingOnGameLaunch')?.addEventListener('change', async (e) => {
      launcherSettings.on_game_launch_action = 'keep_open';
      await persistSettings();
      showToast('Settings Saved', 'PlayCraft stays open continuously while games run.');
    });

    document.getElementById('settingCloseButtonAction')?.addEventListener('change', async (e) => {
      launcherSettings.close_button_action = e.target.value;
      launcherSettings.minimize_to_tray = e.target.value === 'minimize_to_tray';
      const trayEl = document.getElementById('settingMinimizeToTray');
      if (trayEl) trayEl.checked = launcherSettings.minimize_to_tray;
      await persistSettings();
      showToast('Settings Saved', `Close Button Action: ${e.target.selectedOptions[0]?.text || e.target.value}`);
    });

    document.getElementById('settingLaunchOnStartup')?.addEventListener('change', async (e) => {
      launcherSettings.launch_on_windows_startup = Boolean(e.target.checked);
      await persistSettings();
      showToast(
        'Windows Startup',
        e.target.checked
          ? 'PlayCraft will start automatically with Windows in the background.'
          : 'PlayCraft removed from Windows startup.'
      );
    });

    // Dedicated Exit PlayCraft Handlers (Always ask confirmation + warning)
    document.getElementById('exitLauncherTopBtn')?.addEventListener('click', () => openExitConfirmation('Top Close (X) Button'));
    document.getElementById('quitAppCompletelyBtn')?.addEventListener('click', () => {
      closeTopLayer(document.getElementById('settingsDialog'));
      openExitConfirmation('Settings Exit Button');
    });

    window.addEventListener('keydown', (e) => {
      if (e.altKey && e.key === 'F4') {
        e.preventDefault();
        openExitConfirmation('Alt+F4 Shortcut');
        return;
      }

      if ((e.ctrlKey || e.metaKey) && (e.key === 'q' || e.key === 'Q')) {
        e.preventDefault();
        openExitConfirmation('Ctrl+Q Shortcut');
        return;
      }

      const bootOverlay = document.getElementById('bootIntroOverlay');
      if (bootOverlay && !bootOverlay.classList.contains('fade-out')) {
        e.preventDefault();
        if (typeof window.__playcraftDismissBoot === 'function') {
          window.__playcraftDismissBoot();
        } else {
          document.getElementById('skipBootBtn')?.click();
        }
        return;
      }

      if (e.key === 'F11') {
        e.preventDefault();
        document.getElementById('fullscreenToggleBtn')?.click();
        return;
      }

      if (e.ctrlKey && e.shiftKey && (e.key === 'O' || e.key === 'o')) {
        e.preventDefault();
        toggleGlobalOverlay();
        return;
      }

      if ((e.ctrlKey || e.metaKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        document.getElementById('searchToggleBtn')?.click();
        return;
      }

      // Check if a modal is currently open
      const openDialog = document.querySelector('dialog.ps5-dialog[open]');
      if (openDialog) {
        if (e.key === 'Escape') {
          e.preventDefault();
          handleSpatialNav('BACK');
          return;
        }

        const activeTag = document.activeElement?.tagName;
        const isEditingText =
          (activeTag === 'INPUT' && document.activeElement.type !== 'checkbox' && document.activeElement.type !== 'radio') ||
          activeTag === 'TEXTAREA';

        if (e.key === 'ArrowDown' || (e.key === 'Tab' && !e.shiftKey)) {
          e.preventDefault();
          handleSpatialNav('DOWN');
        } else if (e.key === 'ArrowUp' || (e.key === 'Tab' && e.shiftKey)) {
          e.preventDefault();
          handleSpatialNav('UP');
        } else if (!isEditingText && e.key === 'ArrowLeft') {
          e.preventDefault();
          handleSpatialNav('LEFT');
        } else if (!isEditingText && e.key === 'ArrowRight') {
          e.preventDefault();
          handleSpatialNav('RIGHT');
        } else if (e.key === 'Enter' && !isEditingText) {
          e.preventDefault();
          handleSpatialNav('ACTIVATE');
        }
        return;
      }

      // Main Screen Navigation (No modal open)
      const activeTag = document.activeElement?.tagName;
      if (activeTag === 'INPUT' || activeTag === 'TEXTAREA') return;

      if (e.key === 'Escape') {
        e.preventDefault();
        handleSpatialNav('BACK');
      } else if (e.key === 'ArrowRight') {
        e.preventDefault();
        handleSpatialNav('RIGHT');
      } else if (e.key === 'ArrowLeft') {
        e.preventDefault();
        handleSpatialNav('LEFT');
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        handleSpatialNav('UP');
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        handleSpatialNav('DOWN');
      } else if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        handleSpatialNav('ACTIVATE');
      } else if (e.key === 'q' || e.key === 'Q') {
        e.preventDefault();
        cycleTopTabs(-1);
      } else if (e.key === 'e' || e.key === 'E') {
        e.preventDefault();
        cycleTopTabs(1);
      } else if (e.key === 'g' || e.key === 'G') {
        e.preventDefault();
        renderLibraryFullGrid();
        openModal('libraryGridDialog');
      } else if (e.key === 'm' || e.key === 'M') {
        e.preventDefault();
        openModal('mobileDialog');
      } else if (e.key === 's' || e.key === 'S') {
        e.preventDefault();
        openModal('settingsDialog');
      } else if (e.key === 'o' || e.key === 'O') {
        e.preventDefault();
        toggleGlobalOverlay();
      } else if (e.key === 'f' || e.key === 'F') {
        e.preventDefault();
        toggleFavoriteSelectedGame();
      }
    });
  }

  async function openExternalReleaseUrl(url) {
    const target = url || 'https://github.com/RashminaFdo/playcraft/releases/latest';
    try {
      await tauriInvoke('open_external_url', { url: target });
    } catch (_) {
      window.open(target, '_blank', 'noopener');
    }
  }

  function applyUpdateUi(info, manualTrigger = false) {
    if (!info) return;
    latestAppUpdateInfo = { ...latestAppUpdateInfo, ...info };

    const verEl = document.getElementById('aboutInstalledVersion');
    const statusEl = document.getElementById('aboutUpdateStatus');
    const pillBtn = document.getElementById('updateAvailablePill');
    const bannerEl = document.getElementById('updateArrivalBanner');
    const bannerTitle = document.getElementById('updateBannerTitle');
    const bannerMsg = document.getElementById('updateBannerMessage');

    if (verEl && info.current_version) {
      verEl.textContent = `v${info.current_version}`;
    }

    if (statusEl) {
      if (info.has_update) {
        statusEl.textContent = `Update ${info.latest_version} available on GitHub!`;
        statusEl.style.color = 'var(--pc-cyan)';
      } else if (info.offline_or_error) {
        statusEl.textContent = manualTrigger
          ? `Could not check (${info.status_message || 'Offline'})`
          : 'Checks daily in background when online';
        statusEl.style.color = 'var(--ps5-muted)';
      } else if (info.checked_at_ts > 0) {
        const dt = new Date(info.checked_at_ts * 1000);
        statusEl.textContent = `Up to date (Last checked ${dt.toLocaleDateString()})`;
        statusEl.style.color = 'var(--ps5-muted)';
      }
    }

    if (info.has_update) {
      if (pillBtn) {
        pillBtn.textContent = `Update ${info.latest_version}`;
        pillBtn.classList.remove('hidden');
      }
      if (bannerTitle) bannerTitle.textContent = `PlayCraft ${info.latest_version} Has Arrived`;
      if (bannerMsg) {
        bannerMsg.textContent = `You are running v${info.current_version}. Release ${info.latest_version} is ready on GitHub.`;
      }
      if (bannerEl && (!updateBannerDismissed || manualTrigger)) {
        bannerEl.classList.remove('hidden');
        sound.playToastChime();
      }
    } else {
      pillBtn?.classList.add('hidden');
      bannerEl?.classList.add('hidden');
      if (manualTrigger) {
        showToast(
          info.offline_or_error ? 'Update Check Offline' : 'PlayCraft Up to Date',
          info.offline_or_error
            ? (info.status_message || 'Connect to the internet and try again.')
            : `You are running the latest version (v${info.current_version}).`
        );
      }
    }
  }

  async function runAppUpdateCheck(force = false) {
    const checkBtn = document.getElementById('checkUpdatesNowBtn');
    const statusEl = document.getElementById('aboutUpdateStatus');
    if (force && checkBtn) {
      checkBtn.disabled = true;
      checkBtn.textContent = 'Checking...';
    }
    if (force && statusEl) {
      statusEl.textContent = 'Checking GitHub Releases...';
    }
    try {
      const info = await tauriInvoke('check_for_app_update', { force });
      applyUpdateUi(info, force);
    } catch (err) {
      if (force) {
        showToast('Update Check Error', String(err));
      }
    } finally {
      if (checkBtn) {
        checkBtn.disabled = false;
        checkBtn.textContent = 'Check for Updates';
      }
    }
  }

  function refreshUpdateModalWiring() {
    document.getElementById('checkUpdatesNowBtn')?.addEventListener('click', () => {
      sound.playNavTick();
      runAppUpdateCheck(true);
    });

    document.getElementById('openGithubReleaseBtn')?.addEventListener('click', () => {
      sound.playNavTick();
      openExternalReleaseUrl(latestAppUpdateInfo.release_url);
    });

    document.getElementById('updateAvailablePill')?.addEventListener('click', () => {
      sound.playNavTick();
      const bannerEl = document.getElementById('updateArrivalBanner');
      if (bannerEl) {
        updateBannerDismissed = false;
        bannerEl.classList.remove('hidden');
      } else {
        openExternalReleaseUrl(latestAppUpdateInfo.release_url);
      }
    });

    document.getElementById('bannerGetUpdateBtn')?.addEventListener('click', () => {
      sound.playNavTick();
      openExternalReleaseUrl(latestAppUpdateInfo.release_url);
    });

    const hideUpdateBanner = () => {
      updateBannerDismissed = true;
      document.getElementById('updateArrivalBanner')?.classList.add('hidden');
    };
    document.getElementById('dismissUpdateBannerBtn')?.addEventListener('click', hideUpdateBanner);
    document.getElementById('bannerLaterBtn')?.addEventListener('click', hideUpdateBanner);
  }

  // --------------------------------------------------------------------------
  // Initialize PlayCraft Frontend
  // --------------------------------------------------------------------------
  document.addEventListener('DOMContentLoaded', () => {
    setupModernDialogs();
    initBootSequence();
    setupTileStripScrollSync();
    wireUiInteractions();
    refreshUpdateModalWiring();
    startFrameRateCounter();
    loadInitialBackendData();
    startTelemetryPoller();
    startGamepadPoller();

    // Daily silent background update check (~3.5s after launch, on network reconnect & every 6h)
    setTimeout(() => runAppUpdateCheck(false), 3500);
    setInterval(() => runAppUpdateCheck(false), 6 * 60 * 60 * 1000);
    window.addEventListener('online', () => runAppUpdateCheck(false));
  });
})();
