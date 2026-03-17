import http from 'k6/http';
import { check, sleep } from 'k6';
import exec from 'k6/execution';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:3000';
const TOKENS = (__ENV.TOKENS || 'tok_user_1,tok_user_2,tok_user_3,tok_user_4,tok_user_5')
  .split(',')
  .map((value) => value.trim())
  .filter(Boolean);

const POST_IDS = (__ENV.POST_IDS || [
  'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1',
  'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa2',
  'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa3',
  'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa4',
  'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa5',
].join(','))
  .split(',')
  .map((value) => value.trim())
  .filter(Boolean);

const BONUS_HUNTER_IDS = (__ENV.BONUS_HUNTER_IDS || [
  'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1',
  'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb2',
  'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb3',
  'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb4',
  'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb5',
].join(','))
  .split(',')
  .map((value) => value.trim())
  .filter(Boolean);

const TOP_PICKS_IDS = (__ENV.TOP_PICKS_IDS || [
  'cccccccc-cccc-cccc-cccc-ccccccccccc1',
  'cccccccc-cccc-cccc-cccc-ccccccccccc2',
  'cccccccc-cccc-cccc-cccc-ccccccccccc3',
  'cccccccc-cccc-cccc-cccc-ccccccccccc4',
  'cccccccc-cccc-cccc-cccc-ccccccccccc5',
].join(','))
  .split(',')
  .map((value) => value.trim())
  .filter(Boolean);

const CONTENT_FIXTURES = [
  ...POST_IDS.map((contentId) => ({ content_type: 'post', content_id: contentId })),
  ...BONUS_HUNTER_IDS.map((contentId) => ({ content_type: 'bonus_hunter', content_id: contentId })),
  ...TOP_PICKS_IDS.map((contentId) => ({ content_type: 'top_picks', content_id: contentId })),
];

export const options = {
  scenarios: {
    read_path: {
      executor: 'constant-arrival-rate',
      exec: 'readCountScenario',
      rate: Number(__ENV.READ_RATE || 200),
      timeUnit: '1s',
      duration: __ENV.READ_DURATION || '30s',
      preAllocatedVUs: Number(__ENV.READ_VUS || 20),
      maxVUs: Number(__ENV.READ_MAX_VUS || 100),
    },
    batch_path: {
      executor: 'constant-arrival-rate',
      exec: 'batchCountsScenario',
      rate: Number(__ENV.BATCH_RATE || 20),
      timeUnit: '1s',
      duration: __ENV.BATCH_DURATION || '30s',
      preAllocatedVUs: Number(__ENV.BATCH_VUS || 10),
      maxVUs: Number(__ENV.BATCH_MAX_VUS || 50),
      startTime: '2s',
    },
    write_path: {
      executor: 'constant-arrival-rate',
      exec: 'writeLikeScenario',
      rate: Number(__ENV.WRITE_RATE || 10),
      timeUnit: '1s',
      duration: __ENV.WRITE_DURATION || '30s',
      preAllocatedVUs: Number(__ENV.WRITE_VUS || 10),
      maxVUs: Number(__ENV.WRITE_MAX_VUS || 50),
      startTime: '4s',
    },
    mixed_path: {
      executor: 'constant-arrival-rate',
      exec: 'mixedScenario',
      rate: Number(__ENV.MIXED_RATE || 50),
      timeUnit: '1s',
      duration: __ENV.MIXED_DURATION || '30s',
      preAllocatedVUs: Number(__ENV.MIXED_VUS || 20),
      maxVUs: Number(__ENV.MIXED_MAX_VUS || 100),
      startTime: '6s',
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],
    'http_req_duration{endpoint:read_count}': ['p(99)<10'],
    'http_req_duration{endpoint:batch_counts}': ['p(99)<50'],
    'http_req_duration{endpoint:write_like}': ['p(99)<100'],
  },
};

export function setup() {
  for (const token of TOKENS) {
    for (const item of CONTENT_FIXTURES.slice(0, 3)) {
      http.post(`${BASE_URL}/v1/likes`, JSON.stringify(item), {
        headers: authHeaders(token),
        tags: { endpoint: 'warmup_like' },
      });
    }
  }

  return { baseUrl: BASE_URL };
}

export function readCountScenario(data) {
  const item = randomItem();
  const response = http.get(
    `${data.baseUrl}/v1/likes/${item.content_type}/${item.content_id}/count`,
    {
      tags: { endpoint: 'read_count' },
    },
  );

  check(response, {
    'read count status is 200': (r) => r.status === 200,
  });
}

export function batchCountsScenario(data) {
  const batchSize = Number(__ENV.BATCH_SIZE || 50);
  const items = Array.from({ length: batchSize }, () => randomItem());
  const response = http.post(
    `${data.baseUrl}/v1/likes/batch/counts`,
    JSON.stringify({ items }),
    {
      headers: jsonHeaders(),
      tags: { endpoint: 'batch_counts' },
    },
  );

  check(response, {
    'batch counts status is 200': (r) => r.status === 200,
    'batch counts returns all items': (r) => {
      const body = safeJson(r);
      return Array.isArray(body?.results) && body.results.length === batchSize;
    },
  });
}

export function writeLikeScenario(data) {
  const item = randomItem();
  const token = tokenForIteration();
  const response = http.post(
    `${data.baseUrl}/v1/likes`,
    JSON.stringify(item),
    {
      headers: authHeaders(token),
      tags: { endpoint: 'write_like' },
    },
  );

  check(response, {
    'write like status is 200 or 201': (r) => r.status === 200 || r.status === 201,
  });
}

export function mixedScenario(data) {
  const roll = Math.random();

  if (roll < 0.80) {
    readCountScenario(data);
  } else if (roll < 0.95) {
    batchCountsScenario(data);
  } else {
    writeLikeScenario(data);
  }

  sleep(Number(__ENV.MIXED_SLEEP_SECONDS || 0));
}

function randomItem() {
  return CONTENT_FIXTURES[Math.floor(Math.random() * CONTENT_FIXTURES.length)];
}

function tokenForIteration() {
  const iteration = exec.scenario.iterationInTest;
  return TOKENS[iteration % TOKENS.length];
}

function jsonHeaders() {
  return {
    Accept: 'application/json',
    'Content-Type': 'application/json',
  };
}

function authHeaders(token) {
  return {
    ...jsonHeaders(),
    Authorization: `Bearer ${token}`,
  };
}

function safeJson(response) {
  try {
    return response.json();
  } catch (_) {
    return null;
  }
}
