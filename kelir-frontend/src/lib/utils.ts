import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

/**
 * Merges class lists, letting later Tailwind utilities win over earlier ones.
 *
 * Without this, `cn('p-2', 'p-4')` would emit both and the winner would depend
 * on CSS order rather than call order. shadcn-vue components expect it to exist
 * at this path.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs))
}
