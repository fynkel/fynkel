// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  // The API reference's packages each have a page; /api/ is the first.
  redirects: { '/api': '/api/fynkel/' },
  site: 'https://fynkel.com',
  integrations: [
    starlight({
      title: 'Fynkel',
      logo: { src: './src/assets/logo.svg' },
      // The picture a link to the site shows in chats and social posts
      // (also GitHub's social preview): docs/brand has the logo.
      head: [
        { tag: 'meta', attrs: { property: 'og:image', content: 'https://fynkel.com/social.jpg' } },
        { tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
      ],
      description: 'Code-first 3D games in Flutter: write the game in Dart, the way Unity games are written.',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/fynkel/fynkel' }],
      editLink: { baseUrl: 'https://github.com/fynkel/fynkel/edit/main/website/' },
      customCss: ['@fontsource-variable/familjen-grotesk', './src/styles.css'],
      components: { Hero: './src/components/Hero.astro' },
      sidebar: [
        {
          label: 'Start here',
          items: [
            'start/install',
            'start/what-works',
            'start/from-unity',
            'start/existing-app',
            'start/examples',
            'start/troubleshooting',
            'start/changelog',
          ],
        },
        { label: 'Tutorials', items: ['tutorials/roll', 'tutorials/starfall'] },
        {
          label: 'The basics',
          collapsed: true,
          items: [
            'basics/hello',
            'basics/objects',
            'basics/behaviours',
            'basics/lifecycle',
            'basics/time',
            'basics/moving',
            'basics/tweens',
            'basics/routines',
            'basics/signals',
            'basics/errors',
            'basics/pooling',
            'basics/structure',
          ],
        },
        {
          label: 'Input',
          collapsed: true,
          items: [
            'input/reading',
            'input/on-screen',
            'input/pause',
          ],
        },
        {
          label: 'Graphics',
          collapsed: true,
          items: [
            'graphics/meshes-and-materials',
            'graphics/lights',
            'graphics/shadows',
            'graphics/sky-and-fog',
            'graphics/cameras',
            'graphics/looks',
            'graphics/see-through',
            'graphics/bloom-and-ao',
            'graphics/quality',
            'graphics/layers-and-debug',
          ],
        },
        {
          label: 'Shaders',
          collapsed: true,
          items: [
            'shaders/first',
            'shaders/vertices-and-styles',
            'shaders/effects',
            'shaders/in-practice',
            'reference/shader-names',
          ],
        },
        {
          label: 'Assets and shapes',
          collapsed: true,
          items: [
            'assets/loading',
            'assets/meshes',
            'assets/shapes',
            'assets/text-3d',
            'assets/splines',
            'assets/flat-text',
            'assets/sprites',
            'assets/widget-pictures',
          ],
        },
        {
          label: 'Animation',
          collapsed: true,
          items: [
            'animation/clips',
            'animation/skinned',
            'animation/blends',
            'animation/character',
            'animation/crowds',
          ],
        },
        {
          label: 'Sound',
          collapsed: true,
          items: [
            'sound/playing',
            'sound/in-3d',
            'sound/music',
          ],
        },
        {
          label: 'Particles',
          collapsed: true,
          items: [
            'particles/basics',
            'particles/look',
            'particles/world',
            'particles/cost',
          ],
        },
        {
          label: 'Physics',
          collapsed: true,
          items: [
            'physics/colliders',
            'physics/rigidbodies',
            'physics/callbacks',
            'physics/queries',
            'physics/characters',
            'physics/joints',
            'physics/bullets-and-dragging',
          ],
        },
        {
          label: 'Flutter',
          collapsed: true,
          items: [
            'flutter/hud',
            'flutter/menus',
            'flutter/theme',
            'flutter/world-widgets',
            'flutter/in-an-app',
            'flutter/rebinding',
            'flutter/several-views',
          ],
        },
        {
          label: 'Making a game',
          collapsed: true,
          items: [
            'making/hot-reload',
            'making/levels',
            'making/saving',
            'making/testing',
            'making/testing-the-app',
            'making/speed',
            'making/measuring',
            'making/packages',
          ],
        },
        {
          label: 'Recipes',
          collapsed: true,
          items: ['recipes/moving', 'recipes/cameras', 'recipes/fighting', 'recipes/screens', 'recipes/world'],
        },
        {
          label: 'Releasing',
          collapsed: true,
          items: [
            'release/before',
            'release/ios',
            'release/android',
            'release/macos',
          ],
        },
        {
          label: 'API reference',
          items: [
            { label: 'fynkel', link: '/api/fynkel/' },
            { label: 'fynkel_flutter', link: '/api/fynkel_flutter/' },
            { label: 'fynkel_physics', link: '/api/fynkel_physics/' },
            { label: 'fynkel_audio', link: '/api/fynkel_audio/' },
          ],
        },
      ],
    }),
  ],
});
