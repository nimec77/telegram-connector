const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const page = fs.readFileSync(path.join(__dirname, '../src/service/account.html'), 'utf8');
const script = page.match(/<script>([\s\S]*?)<\/script>/)[1];

function fixture() {
  const elements = new Map();
  const requests = [];
  const element = (id) => {
    if (!elements.has(id)) elements.set(id, {
      value: '', checked: false, disabled: id === 'save',
      replaceChildren(...children) { this.children = children; },
    });
    return elements.get(id);
  };
  const response = (value) => ({ ok: true, json: async () => value });
  vm.runInNewContext(script, {
    document: { getElementById: element, createElement: () => ({}) },
    fetch: (url, options) => {
      if (url === '/v1/session') return Promise.resolve(response({ kind: 'direct' }));
      return new Promise(resolve => {
        requests.push({ url, options, resolve: value => resolve(response(value)) });
      });
    },
    encodeURIComponent,
  });
  return { element, requests };
}

function grant(channel, sendEnabled) {
  return {
    generation: 'linked-account',
    revision: 1,
    read: { kind: 'channels', channel_ids: [channel] },
    send_enabled: sendEnabled,
  };
}

test('out-of-order permission loads retain the selected client snapshot', async () => {
  const { element, requests } = fixture();
  element('client').value = 'first';
  const first = element('load').onclick();
  element('client').value = 'second';
  element('client').oninput();
  const second = element('load').onclick();

  requests[1].resolve({ ...grant('42', false), group_refs: ['chat:42'] });
  await second;
  requests[0].resolve(grant('99', true));
  await first;

  assert.equal(element('channels').value, '42');
  assert.equal(element('groups').value, 'chat:42');
  assert.equal(element('send').checked, false);
  assert.equal(element('save').disabled, false);
  const saving = element('grant').onsubmit({ preventDefault() {} });
  assert.equal(requests[2].url, '/v1/account/grants/second');
  assert.deepEqual(JSON.parse(requests[2].options.body), {
    generation: 'linked-account',
    expected_revision: 1,
    read: { kind: 'channels', channel_ids: ['42'] },
    send_enabled: false,
    group_refs: ['chat:42'],
  });
  requests[2].resolve({ ...grant('42', false), revision: 2 });
  await saving;
});

test('loading disables submission until that load settles', async () => {
  const { element, requests } = fixture();
  element('client').value = 'reader';
  const initial = element('load').onclick();
  requests[0].resolve(grant('42', false));
  await initial;

  const refresh = element('load').onclick();
  assert.equal(element('save').disabled, true);
  await element('grant').onsubmit({ preventDefault() {} });
  assert.equal(requests.length, 2);
  requests[1].resolve(grant('43', false));
  await refresh;
  assert.equal(element('channels').value, '43');
  assert.equal(element('save').disabled, false);
});


test('joined channels can be selected from later pages', async () => {
  const { element, requests } = fixture();
  const first = element('list-channels').onclick();
  assert.equal(requests[0].url, '/v1/account/channels?offset=0&limit=100');
  requests[0].resolve({ channels: [{ name: 'First', channel_id: '42' }], total: 101 });
  await first;
  assert.equal(element('previous-channels').hidden, true);
  assert.equal(element('next-channels').hidden, false);

  const next = element('next-channels').onclick();
  assert.equal(requests[1].url, '/v1/account/channels?offset=100&limit=100');
  requests[1].resolve({ channels: [{ name: 'Later', channel_id: '73' }], total: 101 });
  await next;
  assert.deepEqual(element('joined').children.map(item => item.textContent), ['Later (73)']);
  assert.equal(element('previous-channels').hidden, false);
  assert.equal(element('next-channels').hidden, true);
});
