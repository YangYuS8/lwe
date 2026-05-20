import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'LWE',
  description: 'Linux Wallpaper Engine documentation',
  base: '/lwe/',
  cleanUrls: true,
  lastUpdated: true,
  metaChunk: true,
  themeConfig: {
    logo: '/logo.svg',
    socialLinks: [
      { icon: 'github', link: 'https://github.com/YangYuS8/lwe' }
    ],
    search: {
      provider: 'local'
    }
  },
  locales: {
    root: {
      label: 'English',
      lang: 'en-US',
      title: 'LWE',
      description: 'Linux Wallpaper Engine documentation',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/guide/installation' },
          { text: 'Contributing', link: '/contributing/project' }
        ],
        sidebar: [
          {
            text: 'User guide',
            items: [
              { text: 'Introduction', link: '/' },
              { text: 'Installation', link: '/guide/installation' },
              { text: 'Quick start', link: '/guide/quick-start' },
              { text: 'Usage guide', link: '/guide/usage' },
              { text: 'Troubleshooting', link: '/guide/troubleshooting' }
            ]
          },
          {
            text: 'Contributors',
            items: [
              { text: 'Project overview', link: '/contributing/project' },
              { text: 'v1 roadmap', link: '/contributing/roadmap' },
              { text: 'Contributor guide', link: '/contributing/guide' }
            ]
          }
        ],
        outline: {
          label: 'On this page'
        },
        docFooter: {
          prev: 'Previous page',
          next: 'Next page'
        },
        lastUpdated: {
          text: 'Last updated'
        },
        langMenuLabel: 'Change language',
        returnToTopLabel: 'Return to top',
        sidebarMenuLabel: 'Menu',
        darkModeSwitchLabel: 'Appearance',
        lightModeSwitchTitle: 'Switch to light theme',
        darkModeSwitchTitle: 'Switch to dark theme'
      }
    },
    zh: {
      label: '简体中文',
      lang: 'zh-CN',
      title: 'LWE',
      description: 'Linux Wallpaper Engine 文档',
      link: '/zh/',
      themeConfig: {
        nav: [
          { text: '指南', link: '/zh/guide/installation' },
          { text: '贡献', link: '/zh/contributing/project' }
        ],
        sidebar: [
          {
            text: '用户指南',
            items: [
              { text: '简介', link: '/zh/' },
              { text: '安装', link: '/zh/guide/installation' },
              { text: '快速开始', link: '/zh/guide/quick-start' },
              { text: '使用指南', link: '/zh/guide/usage' },
              { text: '故障排查', link: '/zh/guide/troubleshooting' }
            ]
          },
          {
            text: '贡献者',
            items: [
              { text: '项目概览', link: '/zh/contributing/project' },
              { text: 'v1 路线图', link: '/zh/contributing/roadmap' },
              { text: '贡献指南', link: '/zh/contributing/guide' }
            ]
          }
        ],
        outline: {
          label: '本页目录'
        },
        docFooter: {
          prev: '上一页',
          next: '下一页'
        },
        lastUpdated: {
          text: '最后更新'
        },
        langMenuLabel: '切换语言',
        returnToTopLabel: '返回顶部',
        sidebarMenuLabel: '菜单',
        darkModeSwitchLabel: '外观',
        lightModeSwitchTitle: '切换到浅色模式',
        darkModeSwitchTitle: '切换到深色模式'
      }
    }
  }
})
