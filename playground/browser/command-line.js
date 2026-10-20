export function commandLineForArguments(executable, argumentsText) {
  return argumentsText.trim() ? `"${executable}" ${argumentsText}` : undefined;
}
