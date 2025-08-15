// animation/lazydisposal.ts
// Lazy disposal implementation that skips work when possible

import { ZeroCopyCanvasDisplay } from '../canvas/zerocopydisplay.js';

export type Frame = {
  x: number;
  y: number; 
  width: number;
  height: number;
  transparent_index: number | null;
  disposal: number;
  delay: number;
  // ... other frame properties
};

export type BackgroundColor = {
  r: number;
  g: number; 
  b: number;
  a: number;
};

export class LazyDisposalAnimator {
  private prevFrame: Frame | null = null;
  private prevRect: { x: number; y: number; w: number; h: number } | null = null;
  private prevOpaque = false;  // true iff transparent_index == null for prev frame
  private display: ZeroCopyCanvasDisplay;
  private background: BackgroundColor;
  
  // Optional: backup canvas for disposal method 3 (restore to previous)
  private backupCanvas: HTMLCanvasElement | null = null;
  private backupCtx: CanvasRenderingContext2D | null = null;
  private hasBackup = false;
  
  constructor(display: ZeroCopyCanvasDisplay, background: BackgroundColor = { r: 0, g: 0, b: 0, a: 0 }) {
    this.display = display;
    this.background = background;
  }
  
  /**
   * Call after drawing frame k into the composed canvas
   */
  afterPresent(current: Frame): void {
    // Record what we just drew
    this.prevFrame = current;
    this.prevRect = { x: current.x, y: current.y, w: current.width, h: current.height };
    this.prevOpaque = (current.transparent_index === null);
    
    // If this frame has disposal=3 (restore to previous), we might need a backup
    if (current.disposal === 3) {
      this.createBackupIfNeeded();
    } else {
      // Don't need backup for other disposal methods
      this.hasBackup = false;
    }
  }
  
  /**
   * Call before drawing frame k+1; decide whether to actually do disposal for k
   */
  maybeDispose(next: Frame): void {
    if (!this.prevFrame || !this.prevRect) return;

    const pf = this.prevFrame;
    
    // Only disposal methods 2 and 3 require action
    if (pf.disposal === 2 || pf.disposal === 3) {
      // Check if next frame *fully covers* previous rect with opaque pixels
      // If so, we can skip disposal entirely (lazy optimization)
      const nextOpaque = (next.transparent_index === null);
      const covers = this.frameFullyCovers(next, this.prevRect, nextOpaque);

      if (!covers) {
        // Actually need to perform disposal
        if (pf.disposal === 2) {
          this.restoreToBackground(this.prevRect);
        } else if (pf.disposal === 3) {
          this.restoreToPrevious(this.prevRect);
        }
      } else {
        // Skip disposal - next frame will overwrite anyway
        // This saves significant bandwidth for common sticker patterns
      }
    }
    
    // disposal=0 (none) and disposal=1 (keep) require no action
  }
  
  /**
   * Check if nextFrame fully covers rect with opaque pixels
   */
  private frameFullyCovers(
    next: Frame, 
    rect: { x: number; y: number; w: number; h: number }, 
    nextOpaque: boolean
  ): boolean {
    // Conservative: if next has any transparency, don't assume full cover
    if (!nextOpaque) return false;
    
    // Check if next frame bounds fully contain the rect
    return (
      next.x <= rect.x &&
      next.y <= rect.y &&
      (next.x + next.width) >= (rect.x + rect.w) &&
      (next.y + next.height) >= (rect.y + rect.h)
    );
  }
  
  /**
   * Disposal method 2: restore to background color
   */
  private restoreToBackground(rect: { x: number; y: number; w: number; h: number }): void {
    if (this.background.a === 0) {
      // Transparent background - clear to transparent
      this.display.clearRect(rect.x, rect.y, rect.w, rect.h);
    } else {
      // Opaque background - fill with background color
      this.display.fillRect(
        rect.x, rect.y, rect.w, rect.h,
        this.background.r, this.background.g, this.background.b, this.background.a
      );
    }
  }
  
  /**
   * Disposal method 3: restore to previous state
   */
  private restoreToPrevious(rect: { x: number; y: number; w: number; h: number }): void {
    if (this.hasBackup && this.backupCanvas && this.backupCtx) {
      // Restore from backup
      const canvas = this.display['canvas']; // Access private canvas
      const ctx = this.display['ctx'];       // Access private context
      
      ctx.drawImage(
        this.backupCanvas,
        rect.x, rect.y, rect.w, rect.h,  // source rect
        rect.x, rect.y, rect.w, rect.h   // dest rect
      );
    } else {
      // Fallback: restore to background (not spec-compliant but safe)
      this.restoreToBackground(rect);
    }
  }
  
  /**
   * Create backup of current canvas state for disposal method 3
   * Only called when we detect it might be needed
   */
  private createBackupIfNeeded(): void {
    // Only create backup if we might actually need it
    // (i.e., if there's a reasonable chance the next frame won't fully cover)
    const canvas = this.display['canvas'];
    
    if (!this.backupCanvas) {
      this.backupCanvas = document.createElement('canvas');
      this.backupCtx = this.backupCanvas.getContext('2d');
    }
    
    if (this.backupCtx && this.prevRect) {
      // Size backup canvas to just the area we need
      this.backupCanvas.width = this.prevRect.w;
      this.backupCanvas.height = this.prevRect.h;
      
      // Copy just the affected rect (not the whole canvas)
      this.backupCtx.drawImage(
        canvas,
        this.prevRect.x, this.prevRect.y, this.prevRect.w, this.prevRect.h,  // source
        0, 0, this.prevRect.w, this.prevRect.h                              // dest
      );
      
      this.hasBackup = true;
    }
  }
  
  /**
   * Set background color for disposal operations
   */
  setBackground(background: BackgroundColor): void {
    this.background = background;
  }
  
  /**
   * Get current background color
   */
  getBackground(): BackgroundColor {
    return { ...this.background };
  }
  
  /**
   * Reset animator state (e.g., when starting a new GIF)
   */
  reset(): void {
    this.prevFrame = null;
    this.prevRect = null;
    this.prevOpaque = false;
    this.hasBackup = false;
    
    // Clear backup canvas if it exists
    if (this.backupCanvas) {
      this.backupCanvas.width = 0;
      this.backupCanvas.height = 0;
    }
  }
  
  /**
   * Get statistics about disposal optimizations
   */
  getStats(): { framesCovered: number; disposalsSkipped: number } {
    // In a real implementation, you'd track these statistics
    return {
      framesCovered: 0,    // Frames where next fully covered previous
      disposalsSkipped: 0  // Disposal operations that were skipped
    };
  }
}