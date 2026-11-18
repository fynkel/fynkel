// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://fynkel.com',
  integrations: [
    starlight({
      title: 'Fynkel',
      description: 'Code-first 3D games in Flutter: write the game in Dart, the way Unity games are written.',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/mooumari/fynkel' }],
      editLink: { baseUrl: 'https://github.com/mooumari/fynkel/edit/main/website/' },
      sidebar: [
        {
          label: 'Start here',
          items: [
            { label: 'Install and first game', slug: 'start/install' },
            { label: 'Coming from Unity', link: '/guide/22-from-unity/' },
            { label: 'The examples', slug: 'start/examples' },
          ],
        },
        {
          label: 'Tutorials',
          items: [{ label: 'Your first game: Roll', slug: 'tutorials/roll' }],
        },
        { label: 'The guide', items: [{ autogenerate: { directory: 'guide' } }] },
      ],
    }),
  ],
});
