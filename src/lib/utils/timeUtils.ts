/**
 * Formats a duration from total seconds to a MM:SS string.
 * Handles undefined, null, NaN, and negative inputs by returning a placeholder.
 * @param totalSeconds The total duration in seconds.
 * @param placeholder The string to return for invalid inputs (defaults to "--:--").
 * @returns The formatted time string or the placeholder.
 */
export function formatTime(
    totalSeconds: number | undefined | null,
    placeholder: string = "--:--",
): string {
    if (
        totalSeconds === undefined ||
        totalSeconds === null ||
        isNaN(totalSeconds) ||
        totalSeconds < 0
    ) {
        return placeholder;
    }
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = Math.floor(totalSeconds % 60);
    const paddedMinutes = String(minutes).padStart(2, "0");
    const paddedSeconds = String(seconds).padStart(2, "0");
    return `${paddedMinutes}:${paddedSeconds}`;
} 