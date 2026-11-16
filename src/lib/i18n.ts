import { derived, writable } from 'svelte/store';

import type {
  CompatibilityBadge,
  CompatibilityExplanationModel,
  DesktopRestoreState,
  ItemType,
  LibrarySource,
  RuntimeStatus,
  WorkshopSyncStatus
} from '$lib/types';

export type PreferredLanguage = 'en' | 'zh-CN' | 'system';
export type SupportedLocale = 'en' | 'zh-CN';

type CopyFormatValue = string | number;

const dictionaries = {
  en: {
    actionFeedback: {
      failed: 'The action could not be completed.',
      completed: 'The action completed.',
      refreshFailed: 'The action completed, but the latest data could not be loaded: {error}',
      retry: 'Retry',
      retryRefresh: 'Retry refresh'
    },
    appShell: {
      skipToContent: 'Skip to content',
      appDescription: 'A persistent shell for library, workshop, desktop, and settings workflows.',
      primaryLandmark: 'Primary',
      primaryNavigation: 'Primary navigation'
    },
    library: {
      pageTitle: 'Library',
      selectItemLabel: 'Select Library item {itemTitle}',
      navLabel: 'Library',
      navShortLabel: 'Browse',
      navDescription: 'Review local content and current app state.',
      headerTitle: 'Your local library',
      headerSubtitle: 'Browse the content you already own or have synchronized onto this machine.',
      loading: 'Loading Library snapshot…',
      requestError: 'Unable to load the Library request.',
      empty: 'No local wallpapers yet. Subscribe to Wallpaper Engine items in Steam, then refresh your Library after Steam downloads them.',
      niriSessionRequired: 'Wallpaper playback requires a Wayland session with niri. You can still browse your Library and Workshop.',
      monitorDiscoveryUnavailable: 'Monitor discovery is currently unavailable.',
      desktopAssignmentsUnavailable: 'Desktop assignments are currently unavailable.',
      desktopAssignmentDataUnavailable: 'Desktop assignment data is currently unavailable.'
    },
    workshop: {
      pageTitle: 'Workshop',
      navLabel: 'Workshop',
      navShortLabel: 'Sync',
      navDescription: 'Sync Steam Workshop items and refresh directories.',
      headerTitle: 'Steam Workshop discovery',
      headerSubtitle:
        'Search Steam Workshop online, open results in Steam, and mark results already present in Library.',
      localMarkers: 'Local sync markers',
      localMarkersTitle: 'Mark online results already in Library',
      localMarkersDescription:
        'Library owns local content. Workshop uses the local Steam catalog only to mark online results that are already on this machine.',
      localMarkersCount: '{count} local Workshop items are available for online result markers.',
      openLibrary: 'Open Library',
      alreadyLocal: 'In Library',
      onlineOnly: 'Online only',
      refreshCatalog: 'Refresh Catalog',
      refreshingCatalog: 'Refreshing…',
      onlineSearch: 'Online search',
      searchLabel: 'Search query',
      searchPlaceholder: 'Search Steam Workshop',
      searchNow: 'Search',
      ageRatings: 'Age ratings',
      itemTypes: 'Item types',
      pageSize: 'Results per page',
      loadMore: 'Load more',
      noMoreResults: 'No more results',
      showFilters: 'Show filters',
      hideFilters: 'Hide filters',
      previousPage: 'Previous',
      nextPage: 'Next',
      pageLabel: 'Page',
      jumpToPage: 'Jump to',
      goToPage: 'Go',
      onlineResults: 'Online results',
      retainedResults: 'Showing the last successful results. Retry to load the requested search.',
      noOnlineResults: 'No matching online Workshop items were found for the current filters.',
      missingApiKeySettingsHint: 'Online Workshop search needs a Steam Web API key. Open Settings and fill in Steam Web API Key, then return here to search.',
      onlineResultAcquisitionNote: 'Online results are discovery only. Use Steam to subscribe or synchronize content; a visible search result does not mean it is already local.',
      runtimeLabels: {
        runnable: 'Runnable video',
        recognizedOnly: 'Recognized only',
        unsupported: 'Unsupported runtime'
      },
      runtimeDescriptions: {
        video: 'Video items can run after Steam syncs them locally and the desktop runtime is available.',
        scene: 'Scene compatibility is assessed after local sync. Eligible 2D items may use experimental playback; other features remain unavailable.',
        web: 'Web items are recognized for reporting only and do not have a playback backend.',
        application: 'Application items are outside the current runtime scope.'
      },
      syncStatusDescriptions: {
        synced: 'Project metadata and the primary asset were found locally.',
        missing_project: 'The Workshop directory exists, but project.json is missing or malformed.',
        missing_asset: 'Project metadata exists, but the primary wallpaper asset is missing.',
        unsupported_type: 'The item is recognized, but this runtime cannot run its type.'
      },
      ageRatingLabels: {
        g: 'G',
        pg_13: 'PG-13',
        r_18: 'R-18'
      },
      loading: 'Loading Workshop snapshot…',
      requestError: 'Unable to complete the Workshop request.',
      empty: 'No Workshop items are available in the current snapshot.'
    },
    desktop: {
      pageTitle: 'Desktop',
      navLabel: 'Desktop',
      navShortLabel: 'Output',
      navDescription: 'Review monitor output and restore status.',
      headerTitle: 'Monitor shell',
      headerSubtitle:
        'Render the current desktop snapshot without inventing runtime behavior in the frontend.',
      loading: 'Loading Desktop snapshot…',
      requestError: 'Unable to load the Desktop snapshot.',
      view: 'View',
      filterAriaLabel: 'Monitor view filter',
      filterOptions: {
        all: 'All outputs',
        active: 'Current monitors',
        missing: 'Missing restores'
      },
      filterEmptyActive: 'No active monitors are available in the current snapshot.',
      filterEmptyMissing: 'No missing monitor restores are recorded in the current snapshot.',
      monitorsDiscovered: 'Monitors discovered',
      monitorDiscovery: 'Monitor discovery',
      assignmentPersistence: 'Assignment persistence',
      snapshotStale: 'Snapshot stale',
      yes: 'yes',
      no: 'no',
      discoveryUnavailable: 'Monitor discovery is currently unavailable.',
      niriSessionRequired: 'Wallpaper playback requires a Wayland session with niri. You can still browse your Library and Workshop.',
      assignmentPersistenceUnavailable: 'Desktop assignment persistence is currently unavailable.',
      noSavedAssignment: 'No saved assignment',
      empty: 'No monitors are available in the current snapshot.',
      snapshotUnavailable: 'Desktop monitor data is currently unavailable in this snapshot.',
      activeOutputs: 'Active outputs',
      missingMonitorRestores: 'Missing monitor restores',
      runtimeDeferred:
        'The runtime control surface stays deferred until a later task exposes real commands.'
    },
    settings: {
      navLabel: 'Settings',
      navShortLabel: 'Config',
      navDescription: 'Adjust language, theme, and startup behavior.',
      pageTitle: 'Settings',
      headerTitle: 'App preferences',
      headerSubtitle: 'Choose how LWE looks and behaves when you start your desktop session.',
      loading: 'Loading Settings…',
      requestError: 'Unable to complete the Settings request.',
      currentSettings: 'Current settings',
      editableSettings: 'Editable settings',
      preferences: 'Preferences',
      reviewSubtitle: 'Review the current language, theme, and startup behavior before making changes.',
      editSubtitle: 'Update language, theme, and startup behavior, then save when you are ready.',
      language: 'Language',
      theme: 'Theme',
      fpsLimit: 'Wallpaper frame rate',
      fpsLimitHint: 'Choose 30 FPS to save power or 60 FPS for smoother motion. The limit applies to video and Scene wallpapers.',
      compositorPacing: 'Follow display refresh rate',
      fpsValue: '{fps} FPS',
      pauseOnBattery: 'Pause on battery power',
      pauseOnBatteryHint: 'Pause animated wallpapers while the battery is discharging. Playback resumes on AC power unless you paused it manually.',
      playbackSettingsIssue: 'Playback settings were saved, but could not be applied to the running wallpaper.',
      retryPlaybackSettings: 'Retry playback settings',
      launchOnLogin: 'Launch on login',
      steamWebApiKey: 'Steam Web API key',
      steamWebApiKeyHint: 'Required for online Workshop search via Steam Web API QueryFiles. LWE never downloads Workshop content directly.',
      steamWebApiKeySaved: 'Steam Web API key:',
      maskedKeyUnavailable: 'Not set',
      launchOnLoginUnavailable: 'Launch-on-login is currently unavailable on this machine.',
      launchPreferencePrefix: 'Saved preference:',
      startOnSession: 'Start LWE automatically when the graphical desktop session begins.',
      saveChanges: 'Save changes',
      saving: 'Saving…',
      cancel: 'Cancel',
      editSettings: 'Edit settings',
      unsavedChanges: 'Unsaved changes',
      steamIntegration: 'Steam integration',
      currentState: 'Current state',
      steamRequired: 'Steam is required',
      steamOptional: 'Steam is optional',
      savedLanguage: 'Saved language:',
      savedTheme: 'Saved theme:',
      diagnostics: 'Diagnostics',
      diagnosticsTitle: 'Copyable diagnostics',
      diagnosticsDescription:
        'Generate a support bundle summary for bug reports. It includes environment, monitor, Steam, Workshop, Library, and runtime state, but never includes the raw Steam Web API key.',
      generateDiagnostics: 'Generate diagnostics',
      loadingDiagnostics: 'Generating…',
      copyDiagnostics: 'Copy diagnostics',
      diagnosticsCopied: 'Copied',
      launchOnLoginSaved: 'Launch on login:',
      savedLaunchPreference: 'Saved launch preference:',
      enabled: 'enabled',
      disabled: 'disabled',
      preferEnabled: 'Prefer enabled when available',
      preferDisabled: 'Prefer disabled when available',
      languageOptions: {
        en: 'English',
        'zh-CN': 'Simplified Chinese',
        system: 'Follow system locale'
      },
      themeOptions: {
        system: 'Follow system theme',
        light: 'Light',
        dark: 'Dark'
      }
    },
    labels: {
      compatibilityBadges: {
        fully_supported: 'Fully Supported',
        partially_supported: 'Partially Supported',
        unsupported: 'Unsupported'
      },
      itemTypes: {
        video: 'Video',
        scene: 'Scene',
        web: 'Web',
        application: 'Application',
        other: 'Other'
      },
      workshopSyncStatuses: {
        synced: 'Synced',
        missing_project: 'Missing Project',
        missing_asset: 'Missing Asset',
        unsupported_type: 'Unsupported Type'
      },
      librarySources: {
        local: 'Local',
        workshop: 'Workshop',
        core: 'Core',
        other: 'Other'
      }
    },
    components: {
      itemActionsMenu: {
        triggerAriaLabel: 'Show quick actions for {itemTitle}',
        title: 'Quick actions',
        applyFromDetails: 'Apply from details',
        applyFromDetailsDescription:
          'Selects this item so you can choose a monitor and apply it from the detail panel.'
      },
      compatibilityPanel: {
        ariaLabel: 'Compatibility explanation',
        title: 'Compatibility',
        nextStepPrefix: 'Next step: ',
        scene: {
          experimentalHeadline: 'Experimental Scene',
          experimentalDetail: 'This item passed asset validation for experimental 2D Scene playback. It still needs a working desktop and GPU runtime; full Wallpaper Engine compatibility is not established.',
          unsupportedHeadline: 'Scene features unavailable',
          unsupportedDetail: 'This Scene item remains in Library, but cannot play because it uses an unavailable feature. The first blocking feature is listed below.',
          failedHeadline: 'Scene validation failed',
          failedDetail: 'LWE could not validate this Scene item’s resources. Playback is disabled; the resource error is listed below.',
          blockers: 'Playback blockers',
          resourcesNextStep: 'Check the local files and original Wallpaper Engine assets, then refresh the Library.',
          featuresNextStep: 'This item needs additional Scene runtime features before it can play.',
          features: {
            unsupported_script: 'SceneScript',
            unsupported_properties: 'Unsupported or invalid user property bindings',
            unsupported_particles: 'Particle systems',
            unsupported_audio: 'Audio playback or audio-reactive effects',
            unsupported_text: 'Text layers',
            unsupported_model: '3D models or lighting',
            unsupported_puppet: 'Puppet animation',
            unsupported_animated_texture: 'Animated TEXS textures',
            unsupported_video_texture: 'Video textures',
            unsupported_hdr: 'HDR or global bloom',
            unsupported_camera: 'Automatic, tilted or moving camera, or parallax',
            unsupported_hierarchy: 'Layer hierarchy or parent bindings',
            unsupported_shader: 'Shader or effect features',
            unsupported_scene: 'Scene feature outside the current renderer',
            missing_resource: 'Missing scene resource',
            unsafe_resource: 'Unsafe resource path',
            invalid_package: 'Invalid scene package',
            invalid_texture: 'Invalid texture',
            resource_limit: 'Resource size limit',
            resource_access: 'Resource access failed',
            validation_failed: 'Scene validation failed'
          }
        }
      },
      coverImage: {
        placeholderAriaLabel: '{label} placeholder',
        noCover: 'No Cover',
        placeholderDescription: 'Artwork will appear here when available.'
      },
      sceneProperties: {
        title: 'Scene properties',
        description: 'Saving changes or resetting defaults applies this wallpaper to the selected monitor.',
        loading: 'Loading scene properties…',
        empty: 'This scene has no supported editable properties.',
        unnamedGroup: 'Property group {number}',
        emptyGroup: 'This group has no editable properties.',
        save: 'Save and apply',
        saving: 'Saving and applying…',
        reset: 'Reset defaults',
        saved: 'Properties saved and applied.',
        refreshFailed: 'The properties were applied, but the latest values could not be loaded:',
        invalidSnapshot: 'Invalid scene property data.',
        invalidOutcome: 'Invalid action result.',
        chooseImage: 'Choose image',
        clearImage: 'Use author image',
        authorImage: 'Author image',
        selectedImage: 'Custom image selected',
        importingImage: 'Importing image…',
        invalidImage: 'Invalid image selection.',
        imageFormats: 'Static 8-bit PNG or JPEG without a color profile, up to 32 MiB. Save and apply to use the selected image.',
        invalidValues: 'Enter valid values within the declared limits and options.',
        textInputLimit: 'Use up to 256 UTF-8 bytes and eight lines. Line breaks are the only allowed control characters.'
      },
      libraryDetail: {
        title: 'Library detail',
        loading: 'Loading item details…',
        empty: 'Select a Library item to inspect its current detail payload.',
        assignedMonitors: 'Assigned monitors',
        actions: 'Actions',
        apply: 'Apply',
        applyDescription: 'Choose a monitor, then apply this item without leaving Library.',
        applyUnsupported: 'This item is available for compatibility reporting, but its type or resources do not meet the current playback requirements.',
        applySceneUnsupported: 'This Scene wallpaper cannot play with the current resources and features. Review its compatibility report below.',
        monitor: 'Monitor',
        applyTargetMonitor: 'Apply target monitor',
        selectMonitor: 'Select a monitor',
        noMonitorsAvailable: 'No monitors available',
        applying: 'Applying…',
        cover: 'Cover',
        coverDescription: 'Compact artwork preview for quick confirmation without dominating the panel.',
        description: 'Description',
        noDescription: 'No description available for this item yet.',
        tags: 'Tags',
        noTags: 'No tags are attached to this item.',
        source: 'Source',
        itemTitle: 'Library item',
        actionsAriaLabel: 'Apply this item to a monitor'
      },
      workshopDetail: {
        title: 'Workshop detail',
        loading: 'Loading item details…',
        empty: 'Select a Workshop item to inspect its current detail payload.',
        itemTitle: 'Workshop item',
        quickStatus: 'Quick status',
        syncStatusPrefix: 'Sync status:',
        compatibilityPrefix: 'Compatibility:',
        actions: 'Actions',
        openSourcePage: 'Open Steam Workshop page',
        openSourceDescription:
          'Jump to Steam for subscription, comments, and other Workshop context.',
        openInSteam: 'Open In Steam',
        cover: 'Cover',
        coverDescription: 'Compact artwork preview for quick scanning inside the denser Workshop detail flow.',
        description: 'Description',
        noDescription: 'No description is available for this Workshop item yet.',
        tags: 'Tags',
        noTags: 'No tags are attached to this item.',
        syncState: 'Sync state'
      },
      desktopMonitorCard: {
        ariaLabel: 'Desktop monitor {displayName}',
        desktopBadge: 'Desktop',
        statusBadges: {
          runtimeStatuses: {
            running: 'Running',
            idle: 'Idle',
            unsupported: 'Unsupported',
            error: 'Error'
          },
          restoreStates: {
            restored: 'Restored',
            missing_monitor: 'Missing Monitor',
            missing_item: 'Missing Item',
            unavailable: 'Unavailable'
          }
        },
        currentItemLabel: '{displayName} current item',
        monitor: 'Monitor',
        currentItem: 'Current item',
        clearWallpaperAriaLabel: 'Clear wallpaper from {displayName}',
        clear: 'Clear',
        clearing: 'Clearing…',
        restoreState: 'Restore state',
        viewStatusDetails: 'View status details',
        hideStatusDetails: 'Hide status details',
        monitorStatus: 'Monitor status',
        noRestoreIssue:
          'This monitor has state metadata available, but no additional restore issue was reported.',
        expandStatusHint: 'Expand this section to review the latest restore status for this display.'
      },
      itemCard: {
        assignedTo: 'Assigned to',
        runtimeRunnable: 'Runnable video',
        runtimeExperimentalScene: 'Experimental Scene',
        runtimeUnavailable: 'Recognized only',
        runtimeUnavailableDetail: 'Review the compatibility report for missing resources and unavailable playback features.'
      }
    }
  },
  'zh-CN': {
    actionFeedback: {
      failed: '操作未能完成。',
      completed: '操作已完成。',
      refreshFailed: '操作已完成，但无法加载最新数据：{error}',
      retry: '重试',
      retryRefresh: '重新刷新'
    },
    appShell: {
      skipToContent: '跳到内容',
      appDescription: '一个常驻的外壳应用，用于串联内容库、创意工坊、桌面和设置流程。',
      primaryLandmark: '主区域',
      primaryNavigation: '主导航'
    },
    library: {
      pageTitle: '内容库',
      selectItemLabel: '选择内容项 {itemTitle}',
      navLabel: '内容库',
      navShortLabel: '浏览',
      navDescription: '查看本地内容与当前应用状态。',
      headerTitle: '本地内容库',
      headerSubtitle: '浏览这台设备上已经拥有或已同步的内容。',
      loading: '正在加载内容库快照…',
      requestError: '无法加载内容库请求。',
      empty: '暂时没有本地壁纸。请在 Steam 订阅 Wallpaper Engine 创意工坊项目，等待 Steam 下载完成后刷新内容库。',
      niriSessionRequired: '壁纸播放需要 Wayland 与 niri 会话。你仍可浏览内容库和创意工坊。',
      monitorDiscoveryUnavailable: '当前无法发现显示器。',
      desktopAssignmentsUnavailable: '当前无法获取桌面分配。',
      desktopAssignmentDataUnavailable: '当前无法获取桌面分配数据。'
    },
    workshop: {
      pageTitle: '创意工坊',
      navLabel: '创意工坊',
      navShortLabel: '同步',
      navDescription: '同步 Steam 创意工坊项目并刷新目录。',
      headerTitle: 'Steam 创意工坊发现',
      headerSubtitle: '在线搜索 Steam 创意工坊，在 Steam 中打开结果，并标记已进入内容库的项目。',
      localMarkers: '本地同步标记',
      localMarkersTitle: '标记已进入库的在线结果',
      localMarkersDescription:
        '内容库负责管理本地内容。创意工坊只使用本地 Steam 目录来标记哪些在线结果已经在这台机器上。',
      localMarkersCount: '{count} 个本地创意工坊项目可用于标记在线结果。',
      openLibrary: '打开内容库',
      alreadyLocal: '已在内容库',
      onlineOnly: '仅在线',
      refreshCatalog: '刷新目录',
      refreshingCatalog: '正在刷新…',
      onlineSearch: '在线搜索',
      searchLabel: '搜索词',
      searchPlaceholder: '搜索 Steam 创意工坊',
      searchNow: '搜索',
      ageRatings: '年龄分级',
      itemTypes: '项目类型',
      pageSize: '每页数量',
      loadMore: '加载更多',
      noMoreResults: '没有更多结果',
      showFilters: '显示筛选',
      hideFilters: '隐藏筛选',
      previousPage: '上一页',
      nextPage: '下一页',
      pageLabel: '第',
      jumpToPage: '跳转到',
      goToPage: '前往',
      onlineResults: '在线结果',
      retainedResults: '正在显示上次成功加载的结果。请重试以加载本次搜索。',
      noOnlineResults: '当前筛选条件下没有匹配的在线工坊项目。',
      missingApiKeySettingsHint: '在线工坊搜索需要 Steam Web API 密钥。请打开设置并填写 Steam Web API Key，然后返回这里搜索。',
      onlineResultAcquisitionNote: '在线结果仅用于发现内容。订阅或同步仍需要通过 Steam 完成；搜索结果可见并不代表内容已经在本机。',
      runtimeLabels: {
        runnable: '可运行视频',
        recognizedOnly: '仅识别',
        unsupported: '运行时不支持'
      },
      runtimeDescriptions: {
        video: '视频项目通过 Steam 同步到本地，并具备桌面运行环境后可播放。',
        scene: '场景项目同步到本地后会逐项评估。通过验证的 2D 项目可尝试实验性播放；其他特性仍不可用。',
        web: '网页项目仅用于报告识别，尚无播放后端。',
        application: '应用程序项目不在当前运行时范围内。'
      },
      syncStatusDescriptions: {
        synced: '已在本地找到项目元数据和主资源。',
        missing_project: '创意工坊目录存在，但 project.json 缺失或格式错误。',
        missing_asset: '项目元数据存在，但主壁纸资源缺失。',
        unsupported_type: '项目可识别，但当前运行时无法运行此类型。'
      },
      ageRatingLabels: {
        g: 'G',
        pg_13: 'PG-13',
        r_18: 'R-18'
      },
      loading: '正在加载创意工坊快照…',
      requestError: '无法完成创意工坊请求。',
      empty: '当前快照中没有可用的创意工坊项目。'
    },
    desktop: {
      pageTitle: '桌面',
      navLabel: '桌面',
      navShortLabel: '输出',
      navDescription: '查看显示器输出和恢复状态。',
      headerTitle: '显示器概览',
      headerSubtitle: '用当前桌面快照查看显示器输出与恢复状态。',
      loading: '正在加载桌面快照…',
      requestError: '无法加载桌面快照。',
      view: '视图',
      filterAriaLabel: '显示器视图筛选',
      filterOptions: {
        all: '全部输出',
        active: '当前显示器',
        missing: '缺失恢复项'
      },
      filterEmptyActive: '当前快照中没有可用的活动显示器。',
      filterEmptyMissing: '当前快照中没有记录缺失显示器的恢复项。',
      monitorsDiscovered: '已发现显示器',
      monitorDiscovery: '显示器发现',
      assignmentPersistence: '分配持久化',
      snapshotStale: '快照是否过期',
      yes: '是',
      no: '否',
      discoveryUnavailable: '当前无法发现显示器。',
      niriSessionRequired: '壁纸播放需要 Wayland 与 niri 会话。你仍可浏览内容库和创意工坊。',
      assignmentPersistenceUnavailable: '当前无法获取桌面分配持久化状态。',
      noSavedAssignment: '没有已保存的分配',
      empty: '当前快照中没有可用的显示器。',
      snapshotUnavailable: '当前快照中暂时无法获取桌面显示器数据。',
      activeOutputs: '活动输出',
      missingMonitorRestores: '缺失显示器恢复项',
      runtimeDeferred: '运行时控制面板仍会延后到后续任务开放真实命令。'
    },
    settings: {
      navLabel: '设置',
      navShortLabel: '配置',
      navDescription: '调整界面语言、主题与启动行为。',
      pageTitle: '设置',
      headerTitle: '应用偏好',
      headerSubtitle: '选择 LWE 在桌面会话启动时的外观和行为。',
      loading: '正在加载设置…',
      requestError: '无法完成设置请求。',
      currentSettings: '当前设置',
      editableSettings: '编辑设置',
      preferences: '偏好',
      reviewSubtitle: '查看当前语言、主题和启动行为，再决定是否修改。',
      editSubtitle: '修改语言、主题和启动行为，准备好后再保存。',
      language: '语言',
      theme: '主题',
      fpsLimit: '壁纸帧率',
      fpsLimitHint: '选择 30 FPS 可节省电量，60 FPS 可获得更流畅的动画。此上限适用于视频和场景壁纸。',
      compositorPacing: '跟随显示器刷新率',
      fpsValue: '{fps} FPS',
      pauseOnBattery: '使用电池时暂停',
      pauseOnBatteryHint: '电池放电时暂停动态壁纸。接通电源后恢复播放，手动暂停的壁纸仍会保持暂停。',
      playbackSettingsIssue: '播放设置已保存，但未能应用到当前运行的壁纸。',
      retryPlaybackSettings: '重试播放设置',
      launchOnLogin: '登录时启动',
      steamWebApiKey: 'Steam Web API 密钥',
      steamWebApiKeyHint: '用于通过 Steam Web API QueryFiles 进行在线工坊搜索。LWE 不会直接下载创意工坊内容。',
      steamWebApiKeySaved: 'Steam Web API 密钥：',
      maskedKeyUnavailable: '未设置',
      launchOnLoginUnavailable: '当前设备暂不支持登录时自动启动。',
      launchPreferencePrefix: '已保存偏好：',
      startOnSession: '在图形桌面会话开始时自动启动 LWE。',
      saveChanges: '保存更改',
      saving: '正在保存…',
      cancel: '取消',
      editSettings: '编辑设置',
      unsavedChanges: '有未保存的更改',
      steamIntegration: 'Steam 集成',
      currentState: '当前状态',
      steamRequired: '需要 Steam',
      steamOptional: 'Steam 可选',
      savedLanguage: '已保存语言：',
      savedTheme: '已保存主题：',
      diagnostics: '诊断',
      diagnosticsTitle: '可复制诊断信息',
      diagnosticsDescription:
        '生成用于问题报告的支持信息摘要。内容包含环境、显示器、Steam、创意工坊、库和运行时状态，但不会包含原始 Steam Web API Key。',
      generateDiagnostics: '生成诊断信息',
      loadingDiagnostics: '正在生成…',
      copyDiagnostics: '复制诊断信息',
      diagnosticsCopied: '已复制',
      launchOnLoginSaved: '登录时启动：',
      savedLaunchPreference: '已保存启动偏好：',
      enabled: '已启用',
      disabled: '已禁用',
      preferEnabled: '可用时优先启用',
      preferDisabled: '可用时优先禁用',
      languageOptions: {
        en: 'English',
        'zh-CN': '简体中文',
        system: '跟随系统语言'
      },
      themeOptions: {
        system: '跟随系统主题',
        light: '浅色',
        dark: '深色'
      }
    },
    labels: {
      compatibilityBadges: {
        fully_supported: '完全支持',
        partially_supported: '部分支持',
        unsupported: '不支持'
      },
      itemTypes: {
        video: '视频',
        scene: '场景',
        web: '网页',
        application: '应用程序',
        other: '其他'
      },
      workshopSyncStatuses: {
        synced: '已同步',
        missing_project: '缺少工坊项目',
        missing_asset: '缺少资源文件',
        unsupported_type: '暂不支持的类型'
      },
      librarySources: {
        local: '本地',
        workshop: '创意工坊',
        core: '核心内容',
        other: '其他'
      }
    },
    components: {
      itemActionsMenu: {
        triggerAriaLabel: '显示 {itemTitle} 的快捷操作',
        title: '快捷操作',
        applyFromDetails: '从详情中应用',
        applyFromDetailsDescription: '先选择这个内容项，然后可以在详情面板中选择显示器并应用。'
      },
      compatibilityPanel: {
        ariaLabel: '兼容性说明',
        title: '兼容性',
        nextStepPrefix: '下一步：',
        scene: {
          experimentalHeadline: '实验性场景',
          experimentalDetail: '此项目已通过实验性 2D 场景播放的资源验证。仍需要可用的桌面与 GPU 运行环境；这不代表完整兼容 Wallpaper Engine。',
          unsupportedHeadline: '场景特性不可用',
          unsupportedDetail: '此场景项目保留在内容库中，但使用了不可用的特性，暂时无法播放。下方列出首次阻断播放的特性。',
          failedHeadline: '场景验证失败',
          failedDetail: 'LWE 无法验证此场景项目的资源，因此禁用播放。下方列出资源错误。',
          blockers: '播放阻断原因',
          resourcesNextStep: '检查本地文件和原版 Wallpaper Engine 的资源，再刷新内容库。',
          featuresNextStep: '此项目需要补充相应的场景运行时特性才能播放。',
          features: {
            unsupported_script: 'SceneScript 脚本',
            unsupported_properties: '不受支持或无效的用户属性绑定',
            unsupported_particles: '粒子系统',
            unsupported_audio: '音频播放或音频响应效果',
            unsupported_text: '文字图层',
            unsupported_model: '3D 模型或光照',
            unsupported_puppet: '骨骼动画',
            unsupported_animated_texture: 'TEXS 动态纹理',
            unsupported_video_texture: '视频纹理',
            unsupported_hdr: 'HDR 或全局泛光',
            unsupported_camera: '自动、倾斜或运动相机，或视差',
            unsupported_hierarchy: '图层层级或父级绑定',
            unsupported_shader: '着色器或特效能力',
            unsupported_scene: '当前渲染器范围之外的场景特性',
            missing_resource: '场景资源缺失',
            unsafe_resource: '不安全的资源路径',
            invalid_package: '场景包格式错误',
            invalid_texture: '纹理格式错误',
            resource_limit: '资源大小超出限制',
            resource_access: '无法访问资源',
            validation_failed: '场景验证失败'
          }
        }
      },
      coverImage: {
        placeholderAriaLabel: '{label} 占位图',
        noCover: '无封面',
        placeholderDescription: '有封面时会在这里显示。'
      },
      sceneProperties: {
        title: '场景属性',
        description: '保存修改或恢复默认值会将此壁纸应用到所选显示器。',
        loading: '正在加载场景属性…',
        empty: '此场景没有受支持的可编辑属性。',
        unnamedGroup: '属性分组 {number}',
        emptyGroup: '此分组没有可编辑属性。',
        save: '保存并应用',
        saving: '正在保存并应用…',
        reset: '恢复默认值',
        saved: '属性已保存并应用。',
        refreshFailed: '属性已应用，但无法加载最新值：',
        invalidSnapshot: '场景属性数据无效。',
        invalidOutcome: '操作结果无效。',
        chooseImage: '选择图片',
        clearImage: '使用作者图片',
        authorImage: '作者图片',
        selectedImage: '已选择自定义图片',
        importingImage: '正在导入图片…',
        invalidImage: '图片选择无效。',
        imageFormats: '支持不含色彩配置的静态 8 位 PNG 或 JPEG，最大 32 MiB。保存并应用后使用所选图片。',
        invalidValues: '请输入声明范围内的有效值，并选择声明的选项。',
        textInputLimit: '最多输入 256 个 UTF-8 字节和 8 行。控制字符只允许换行。'
      },
      libraryDetail: {
        title: '内容详情',
        loading: '正在加载内容详情…',
        empty: '选择一个内容项以查看当前详情。',
        assignedMonitors: '已分配显示器',
        actions: '操作',
        apply: '应用',
        applyDescription: '选择一个显示器，然后直接在内容库中应用这个内容项。',
        applyUnsupported: '此内容项可用于兼容性报告，但其类型或资源不满足当前播放要求。',
        applySceneUnsupported: '此场景壁纸的资源或特性不满足当前播放要求。请查看下方的兼容性报告。',
        monitor: '显示器',
        applyTargetMonitor: '选择要应用到的显示器',
        selectMonitor: '选择一个显示器',
        noMonitorsAvailable: '没有可用的显示器',
        applying: '正在应用…',
        cover: '封面',
        coverDescription: '以紧凑的预览图快速确认内容，而不会占据整个面板。',
        description: '描述',
        noDescription: '这个内容项暂时没有可用描述。',
        tags: '标签',
        noTags: '这个内容项还没有附加标签。',
        source: '来源',
        itemTitle: '内容项',
        actionsAriaLabel: '将这个内容项应用到显示器'
      },
      workshopDetail: {
        title: '工坊详情',
        loading: '正在加载项目详情…',
        empty: '选择一个创意工坊项目以查看当前详情。',
        itemTitle: '工坊项目',
        quickStatus: '快速状态',
        syncStatusPrefix: '同步状态：',
        compatibilityPrefix: '兼容性：',
        actions: '操作',
        openSourcePage: '打开 Steam 创意工坊页面',
        openSourceDescription: '跳转到 Steam 查看订阅、评论以及其他创意工坊上下文。',
        openInSteam: '在 Steam 中打开',
        cover: '封面',
        coverDescription: '在更紧凑的工坊详情流程中提供封面预览，便于快速浏览。',
        description: '描述',
        noDescription: '这个创意工坊项目暂时没有可用描述。',
        tags: '标签',
        noTags: '这个项目还没有附加标签。',
        syncState: '同步状态'
      },
      desktopMonitorCard: {
        ariaLabel: '桌面显示器 {displayName}',
        desktopBadge: '桌面',
        statusBadges: {
          runtimeStatuses: {
            running: '运行中',
            idle: '空闲',
            unsupported: '不支持',
            error: '错误'
          },
          restoreStates: {
            restored: '已恢复',
            missing_monitor: '缺少显示器',
            missing_item: '缺少内容项',
            unavailable: '不可用'
          }
        },
        currentItemLabel: '{displayName} 当前内容',
        monitor: '显示器',
        currentItem: '当前内容',
        clearWallpaperAriaLabel: '从 {displayName} 清除壁纸',
        clear: '清除',
        clearing: '正在清除…',
        restoreState: '恢复状态',
        viewStatusDetails: '查看状态详情',
        hideStatusDetails: '隐藏状态详情',
        monitorStatus: '显示器状态',
        noRestoreIssue: '此显示器存在状态元数据，但没有报告额外的恢复问题。',
        expandStatusHint: '展开此区域以查看这个显示器的最新恢复状态。'
      },
      itemCard: {
        assignedTo: '已分配到',
        runtimeRunnable: '可运行视频',
        runtimeExperimentalScene: '实验性场景',
        runtimeUnavailable: '仅识别',
        runtimeUnavailableDetail: '请查看兼容性报告中的缺失资源和不可用播放特性。'
      }
    }
  }
} as const;

export type CopyDictionary = (typeof dictionaries)[SupportedLocale];

export const resolveLocale = (language: PreferredLanguage): SupportedLocale =>
  language === 'zh-CN' ? 'zh-CN' : 'en';

export const getCopyForLanguage = (language: PreferredLanguage) => dictionaries[resolveLocale(language)];

export const getItemTypeLabel = (copyValue: CopyDictionary, itemType: ItemType) =>
  copyValue.labels.itemTypes[itemType];

export const getWorkshopSyncStatusLabel = (
  copyValue: CopyDictionary,
  syncStatus: WorkshopSyncStatus
) => copyValue.labels.workshopSyncStatuses[syncStatus];

export const getCompatibilityBadgeLabel = (
  copyValue: CopyDictionary,
  badge: CompatibilityBadge
) => copyValue.labels.compatibilityBadges[badge];

export const getSceneIssueLabel = (copyValue: CopyDictionary, code: string) => {
  const labels = copyValue.components.compatibilityPanel.scene.features;
  return labels[code as keyof typeof labels] ?? labels.validation_failed;
};

export const localizeSceneCompatibility = (
  copyValue: CopyDictionary,
  compatibility: CompatibilityExplanationModel
): CompatibilityExplanationModel => {
  const sceneCopy = copyValue.components.compatibilityPanel.scene;
  switch (compatibility.reasonCode) {
    case 'scene_experimental':
      return {
        ...compatibility,
        headline: sceneCopy.experimentalHeadline,
        detail: sceneCopy.experimentalDetail,
        nextStepCopy: null
      };
    case 'scene_unsupported_features':
      return {
        ...compatibility,
        headline: sceneCopy.unsupportedHeadline,
        detail: sceneCopy.unsupportedDetail,
        nextStepCopy: sceneCopy.featuresNextStep
      };
    case 'scene_assessment_failed':
      return {
        ...compatibility,
        headline: sceneCopy.failedHeadline,
        detail: sceneCopy.failedDetail,
        nextStepCopy: sceneCopy.resourcesNextStep
      };
    default:
      return compatibility;
  }
};

export const getLibrarySourceLabel = (copyValue: CopyDictionary, source: LibrarySource) =>
  copyValue.labels.librarySources[source];

export const getDesktopRuntimeStatusLabel = (
  copyValue: CopyDictionary,
  runtimeStatus: RuntimeStatus
) => copyValue.components.desktopMonitorCard.statusBadges.runtimeStatuses[runtimeStatus];

export const getDesktopRestoreStateLabel = (
  copyValue: CopyDictionary,
  restoreState: DesktopRestoreState
) => copyValue.components.desktopMonitorCard.statusBadges.restoreStates[restoreState];

export const formatCopy = (
  template: string,
  values: Record<string, CopyFormatValue>
) =>
  template.replace(/\{(\w+)\}/g, (match, key) => {
    const value = values[key];
    return value === undefined ? match : String(value);
  });

const preferredLanguage = writable<PreferredLanguage>('en');

export const locale = derived(preferredLanguage, ($preferredLanguage): SupportedLocale =>
  resolveLocale($preferredLanguage)
);

export const copy = derived(locale, ($locale) => dictionaries[$locale]);

export const setPreferredLanguage = (language: PreferredLanguage) => {
  preferredLanguage.set(language);
};

export const resetPreferredLanguage = () => {
  preferredLanguage.set('en');
};
