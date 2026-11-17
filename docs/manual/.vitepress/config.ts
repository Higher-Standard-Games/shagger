import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'SHAGGER',
  description: 'The Rust counterpart of the SHAG engine, for native clients',

  srcDir: '.',

  // Published at https://shagger.shag.dev/, beside SHAG's manual at https://shag.dev/
  sitemap: { hostname: 'https://shagger.shag.dev/' },
  outDir: '../../dist-docs',

  // localhost URLs are dev-server instructions, not links to verify
  ignoreDeadLinks: [/^https?:\/\/localhost/],

  head: [
    ['link', { rel: 'preconnect', href: 'https://fonts.googleapis.com' }],
    ['link', { rel: 'preconnect', href: 'https://fonts.gstatic.com', crossorigin: '' }],
  ],

  themeConfig: {
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Getting Started', link: '/getting-started/' },
      { text: 'Guides', link: '/guides/' },
      { text: 'Tools', link: '/tools/' },
      { text: 'SHAG manual', link: 'https://shag.dev/' }
    ],

    sidebar: {
      '/': [
        {
          text: 'Introduction',
          items: [
            { text: 'What is SHAGGER?', link: '/introduction/' }
          ]
        },
        {
          text: 'Getting Started',
          items: [
            { text: 'Build and test', link: '/getting-started/' },
            { text: 'Project Structure', link: '/getting-started/project-structure' }
          ]
        },
        {
          text: 'Guides',
          items: [
            { text: 'Overview', link: '/guides/' },
            {
              text: 'Core',
              collapsed: false,
              items: [
                { text: 'Cvars and Console', link: '/guides/cvars-and-console' },
                { text: 'Input', link: '/guides/input' },
                { text: 'Native Runtime', link: '/guides/native-runtime' },
                { text: 'JavaScript Parity', link: '/guides/js-parity' }
              ]
            },
            {
              text: 'Content',
              collapsed: false,
              items: [
                { text: 'Packages and Maps', link: '/guides/packages-and-maps' },
                { text: 'Models and Animation', link: '/guides/models-and-animation' },
                { text: 'Demos and Highlight Reels', link: '/guides/demos-and-reels' }
              ]
            },
            {
              text: 'Presentation',
              collapsed: false,
              items: [
                { text: 'Rendering', link: '/guides/rendering' },
                { text: 'Audio', link: '/guides/audio' },
                { text: 'UI', link: '/guides/ui' },
                { text: 'Viewport and Weapons', link: '/guides/viewport-and-weapons' }
              ]
            },
            {
              text: 'Gameplay and Multiplayer',
              collapsed: false,
              items: [
                { text: 'Collision and Physics', link: '/guides/collision-and-physics' },
                { text: 'Networking and Lobby', link: '/guides/networking-and-lobby' }
              ]
            }
          ]
        },
        {
          text: 'Tools',
          items: [
            { text: 'Overview', link: '/tools/' },
            { text: 'Golden Vectors', link: '/tools/golden-vectors' }
          ]
        },
        { text: 'FAQ', link: '/faq/' },
        { text: 'Contributing', link: '/contributing/' }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/Higher-Standard-Games/shagger' }
    ],

    search: {
      provider: 'local'
    },

    footer: {
      message: 'Released under the MIT License. Babylon.js ports under Apache-2.0.',
      copyright: 'Copyright © 2026 Higher Standard Games'
    }
  }
})
