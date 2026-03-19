import http from 'k6/http';
import { check, sleep } from 'k6';
import exec from 'k6/execution';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:8080';
const BENCHMARK = (__ENV.BENCHMARK || 'read').toLowerCase();
const RATE_LIMIT_AWARE = (__ENV.RATE_LIMIT_AWARE || 'true') === 'true';
const READ_TARGET_RATE = Number(__ENV.READ_RATE || 10000);
const BATCH_TARGET_RATE = Number(__ENV.BATCH_RATE || 1000);
const WRITE_TARGET_RATE = Number(__ENV.WRITE_RATE || 500);
const MIXED_TARGET_RATE = Number(__ENV.MIXED_RATE || 6666);
const READ_LIMIT_PER_MINUTE = Number(__ENV.RATE_LIMIT_READ_PER_MINUTE || 1000);
const WRITE_LIMIT_PER_MINUTE = Number(__ENV.RATE_LIMIT_WRITE_PER_MINUTE || 30);
const READ_IP_SAFETY_FACTOR = Number(__ENV.READ_IP_SAFETY_FACTOR || 1.2);
const WRITE_SAFETY_FACTOR = Number(__ENV.WRITE_SAFETY_FACTOR || 0.8);
const SCENARIOS = buildScenarios();
const READ_IP_POOL_SIZE = Math.max(
  1,
  Number(__ENV.READ_IP_POOL_SIZE || requiredReadIpPoolSize()),
);
const TOKENS = (__ENV.TOKENS || generateTokens(requiredTokenCount()))
  .split(',')
  .map((value) => value.trim())
  .filter(Boolean);

const POST_IDS = (__ENV.POST_IDS || [
  '731b0395-4888-4822-b516-05b4b7bf2089',
  '9601c044-6130-4ee5-a155-96570e05a02f',
  '933dde0f-4744-4a66-9a38-bf5cb1f67553',
  'ea0f2020-0509-45fd-adb9-24b8843055ee',
  'bd27f926-0a00-41fd-b085-a7491e6d0902',
  '2a656157-5284-48b5-9d76-ede492933347',
  '4f884e5e-2f1d-4965-b0f1-16922acd91a2',
  'ad1d9238-622c-4875-9881-5f8e19997783',
  'c34ee1e3-7224-4a97-ba44-0993eb7a6ed8',
  'c2b7f212-6162-4ae6-837b-16ee34cc9a50',
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
  scenarios: SCENARIOS,
  thresholds: buildThresholds(),
};

export function setup() {
  const warmupTokenCount = Math.max(1, Number(__ENV.WARMUP_TOKEN_COUNT || 5));

  for (const token of TOKENS.slice(0, warmupTokenCount)) {
    for (const item of CONTENT_FIXTURES.slice(0, 3)) {
      http.post(`${BASE_URL}/v1/likes`, JSON.stringify(item), {
        headers: authHeaders(token),
        tags: { endpoint: 'warmup_like' },
      });
    }
  }

  for (const [index, item] of CONTENT_FIXTURES.entries()) {
    http.get(`${BASE_URL}/v1/likes/${item.content_type}/${item.content_id}/count`, {
      headers: warmupPublicHeaders(index),
      tags: { endpoint: 'warmup_count' },
    });
  }

  if (RATE_LIMIT_AWARE) {
    for (let index = 0; index < READ_IP_POOL_SIZE; index += 1) {
      const item = CONTENT_FIXTURES[index % CONTENT_FIXTURES.length];
      http.get(`${BASE_URL}/v1/likes/${item.content_type}/${item.content_id}/count`, {
        headers: warmupPublicHeaders(index),
        tags: { endpoint: 'warmup_read_lease' },
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
      headers: publicHeaders(),
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
      headers: publicHeaders(),
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

function publicHeaders() {
  const headers = jsonHeaders();

  if (RATE_LIMIT_AWARE) {
    headers['X-Forwarded-For'] = syntheticClientIp();
  }

  return headers;
}

function authHeaders(token) {
  return {
    ...jsonHeaders(),
    Authorization: `Bearer ${token}`,
  };
}

function warmupPublicHeaders(index) {
  const headers = jsonHeaders();

  if (RATE_LIMIT_AWARE) {
    headers['X-Forwarded-For'] = syntheticClientIpForIndex(index);
  }

  return headers;
}

function safeJson(response) {
  try {
    return response.json();
  } catch (_) {
    return null;
  }
}

function syntheticClientIp() {
  const iteration = exec.scenario.iterationInTest % READ_IP_POOL_SIZE;
  return syntheticClientIpForIndex(iteration);
}

function syntheticClientIpForIndex(iteration) {
  const secondOctet = 113 + (Math.floor(iteration / 250) % 10);
  const thirdOctet = Math.floor(iteration / 50) % 250;
  const fourthOctet = (iteration % 50) + 1;
  return `203.${secondOctet}.${thirdOctet}.${fourthOctet}`;
}

function buildThresholds() {
  const thresholds = {
    'http_req_duration{endpoint:read_count}': [`p(99)<${Number(__ENV.READ_P99_MS || 10)}`],
    'http_req_duration{endpoint:batch_counts}': [`p(99)<${Number(__ENV.BATCH_P99_MS || 50)}`],
    'http_req_duration{endpoint:write_like}': [`p(99)<${Number(__ENV.WRITE_P99_MS || 100)}`],
  };

  thresholds.http_req_failed = RATE_LIMIT_AWARE ? ['rate<0.01'] : ['rate<0.95'];

  return thresholds;
}

function buildScenarios() {
  switch (BENCHMARK) {
    case 'read':
      return {
        read_path: buildScenario('readCountScenario', READ_TARGET_RATE, 'READ', 200, 1000),
      };
    case 'batch':
      return {
        batch_path: buildScenario('batchCountsScenario', BATCH_TARGET_RATE, 'BATCH', 100, 500),
      };
    case 'write':
      return {
        write_path: buildScenario('writeLikeScenario', WRITE_TARGET_RATE, 'WRITE', 100, 1000),
      };
    case 'mixed':
      return {
        mixed_path: buildScenario('mixedScenario', MIXED_TARGET_RATE, 'MIXED', 200, 1000),
      };
    case 'all':
      return {
        read_path: buildScenario('readCountScenario', READ_TARGET_RATE, 'READ', 200, 1000),
        batch_path: {
          ...buildScenario('batchCountsScenario', BATCH_TARGET_RATE, 'BATCH', 100, 500),
          startTime: '2s',
        },
        write_path: {
          ...buildScenario('writeLikeScenario', WRITE_TARGET_RATE, 'WRITE', 100, 1000),
          startTime: '4s',
        },
        mixed_path: {
          ...buildScenario('mixedScenario', MIXED_TARGET_RATE, 'MIXED', 200, 1000),
          startTime: '6s',
        },
      };
    default:
      throw new Error(`unsupported BENCHMARK value: ${BENCHMARK}`);
  }
}

function buildScenario(execName, defaultRate, prefix, defaultVUs, defaultMaxVUs) {
  return {
    executor: 'constant-arrival-rate',
    exec: execName,
    rate: defaultRate,
    timeUnit: '1s',
    duration: __ENV[`${prefix}_DURATION`] || '30s',
    preAllocatedVUs: Number(__ENV[`${prefix}_VUS`] || defaultVUs),
    maxVUs: Number(__ENV[`${prefix}_MAX_VUS`] || defaultMaxVUs),
  };
}

function requiredReadIpPoolSize() {
  if (!RATE_LIMIT_AWARE) {
    return 1;
  }

  const readCapacityPerIp = Math.max(1, READ_LIMIT_PER_MINUTE / 60);
  const publicReadRate =
    Object.values(SCENARIOS).reduce((sum, scenario) => {
      if (scenario.exec === 'readCountScenario') {
        return sum + scenario.rate;
      }

      if (scenario.exec === 'batchCountsScenario') {
        return sum + scenario.rate;
      }

      if (scenario.exec === 'mixedScenario') {
        return sum + (scenario.rate * 0.95);
      }

      return sum;
    }, 0);

  return Math.ceil((publicReadRate / readCapacityPerIp) * READ_IP_SAFETY_FACTOR);
}

function requiredTokenCount() {
  const writeRate =
    Object.values(SCENARIOS).reduce((sum, scenario) => {
      if (scenario.exec === 'writeLikeScenario') {
        return sum + scenario.rate;
      }

      if (scenario.exec === 'mixedScenario') {
        return sum + (scenario.rate * 0.05);
      }

      return sum;
    }, 0);

  const safeWritesPerUserPerSecond =
    Math.max(1, (WRITE_LIMIT_PER_MINUTE * WRITE_SAFETY_FACTOR) / 60);

  return Math.max(5, Math.ceil(writeRate / safeWritesPerUserPerSecond));
}

function generateTokens(count) {
  return Array.from({ length: count }, (_, index) => `tok_user_${index + 1}`).join(',');
}
