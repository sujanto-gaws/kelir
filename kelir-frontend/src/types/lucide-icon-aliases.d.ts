/**
 * `vite.config.ts`'s `lucideIconAliases` plugin: Lucide's old export names,
 * each mapped to the icon file it now names (`AlertTriangle` →
 * `triangle-alert`). Read by `layouts/menuIcon.ts`.
 */
declare module 'virtual:lucide-icon-aliases' {
  const aliases: Readonly<Record<string, string>>
  export default aliases
}
