/**
 * Abandon the process and log the given error
 *
 * @param {string} error
 */
export function exitWithError(error) {
    console.error(error)
    process.exit(1)
}