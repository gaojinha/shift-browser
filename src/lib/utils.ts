import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

/** shadcn 风格类名合并：clsx 组装 + tailwind-merge 去冲突 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
