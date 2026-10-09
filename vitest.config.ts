import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config.ts';

export default mergeConfig(viteConfig, defineConfig({
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: 'server',
          environment: 'node',
          include: ['src/**/*.test.ts'],
          exclude: ['src/**/*.interaction.test.ts']
        }
      },
      {
        extends: true,
        resolve: { conditions: ['browser'] },
        test: {
          name: 'interaction',
          environment: 'jsdom',
          setupFiles: ['src/lib/testing/interaction-setup.ts'],
          include: ['src/**/*.interaction.test.ts']
        }
      }
    ]
  }
}));
