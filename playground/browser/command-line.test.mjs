import assert from 'node:assert/strict';
import test from 'node:test';
import {commandLineForArguments} from './command-line.js';

test('empty arguments leave the executable command line to the SDK default', () => {
  for (const value of ['', '   ', '\t']) {
    assert.equal(commandLineForArguments('C:\\sample app\\program.exe',value),undefined);
  }
});

test('the selected executable is argv0 and the argument tail is preserved verbatim', () => {
  const executable = 'C:\\sample app\\program.exe';
  const argumentsText = ' -mode "two words" -file C:\\data\\input.bin  ';
  assert.equal(commandLineForArguments(executable,argumentsText),`"${executable}" ${argumentsText}`);
  assert.equal(commandLineForArguments('C:\\program.exe','-flag'),'"C:\\program.exe" -flag');
});
