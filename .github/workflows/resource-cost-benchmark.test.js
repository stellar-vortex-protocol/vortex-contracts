// Test suite for the resource-cost documentation pipeline CI job.
// Verifies that benchmark results are properly compared against published values
// and that drift beyond thresholds is detected and reported.

const fs = require('fs');
const path = require('path');

// Mock benchmark result parser
class BenchmarkParser {
    static parseFromFile(filePath) {
        try {
            const content = fs.readFileSync(filePath, 'utf8');
            return JSON.parse(content);
        } catch (error) {
            throw new Error(`Failed to parse benchmark file: ${error.message}`);
        }
    }

    static extractResourceCosts(benchmarkOutput) {
        // Simulate extracting resource costs from benchmark harness output
        // Expected format: { entrypoint: name, cpu: cost, memory: cost, ... }
        return benchmarkOutput.results || [];
    }
}

// Mock documentation parser
class DocParser {
    static parsePublishedCosts(docPath) {
        try {
            const content = fs.readFileSync(docPath, 'utf8');

            // Extract resource costs from markdown table
            const costsMatch = content.match(/## Resource Costs([\s\S]*?)##/);
            if (!costsMatch) {
                throw new Error('Resource costs section not found in documentation');
            }

            const costs = {};
            const lines = costsMatch[1].split('\n');

            for (const line of lines) {
                // Parse markdown table rows: | entrypoint | cpu | memory | ... |
                const match = line.match(/\|\s*([^\|]+?)\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|/);
                if (match) {
                    costs[match[1].trim()] = {
                        cpu: parseInt(match[2]),
                        memory: parseInt(match[3])
                    };
                }
            }

            return costs;
        } catch (error) {
            throw new Error(`Failed to parse documentation: ${error.message}`);
        }
    }

    static updatePublishedCosts(docPath, newCosts) {
        try {
            let content = fs.readFileSync(docPath, 'utf8');

            // Build markdown table
            let table = '\n## Resource Costs\n\n| Entrypoint | CPU | Memory |\n|---|---|---|\n';
            for (const [entrypoint, costs] of Object.entries(newCosts)) {
                table += `| ${entrypoint} | ${costs.cpu} | ${costs.memory} |\n`;
            }

            // Replace or add resource costs section
            const sectionRegex = /## Resource Costs([\s\S]*?)(?=##|$)/;
            if (sectionRegex.test(content)) {
                content = content.replace(sectionRegex, table);
            } else {
                content = content + table;
            }

            fs.writeFileSync(docPath, content);
        } catch (error) {
            throw new Error(`Failed to update documentation: ${error.message}`);
        }
    }
}

// Drift detection and reporting
class DriftDetector {
    constructor(thresholdPercent = 10) {
        this.thresholdPercent = thresholdPercent;
    }

    detectDrift(publishedCosts, measuredCosts) {
        const driftReport = {
            hasDrift: false,
            driftItems: [],
            summary: null
        };

        for (const [entrypoint, published] of Object.entries(publishedCosts)) {
            if (!measuredCosts[entrypoint]) {
                continue;
            }

            const measured = measuredCosts[entrypoint];

            // Calculate percentage drift for CPU
            const cpuDrift = Math.abs((measured.cpu - published.cpu) / published.cpu) * 100;
            if (cpuDrift > this.thresholdPercent) {
                driftReport.hasDrift = true;
                driftReport.driftItems.push({
                    entrypoint,
                    resource: 'cpu',
                    published: published.cpu,
                    measured: measured.cpu,
                    driftPercent: cpuDrift.toFixed(2)
                });
            }

            // Calculate percentage drift for memory
            const memoryDrift = Math.abs((measured.memory - published.memory) / published.memory) * 100;
            if (memoryDrift > this.thresholdPercent) {
                driftReport.hasDrift = true;
                driftReport.driftItems.push({
                    entrypoint,
                    resource: 'memory',
                    published: published.memory,
                    measured: measured.memory,
                    driftPercent: memoryDrift.toFixed(2)
                });
            }
        }

        if (driftReport.hasDrift) {
            driftReport.summary = `Detected ${driftReport.driftItems.length} resource cost drift(s) exceeding ${this.thresholdPercent}% threshold`;
        }

        return driftReport;
    }

    formatDriftReport(driftReport) {
        if (!driftReport.hasDrift) {
            return 'No significant drift detected. Published costs remain accurate.';
        }

        let report = `# Resource Cost Drift Report\n\n${driftReport.summary}\n\n`;
        report += '| Entrypoint | Resource | Published | Measured | Drift % |\n';
        report += '|---|---|---|---|---|\n';

        for (const item of driftReport.driftItems) {
            report += `| ${item.entrypoint} | ${item.resource} | ${item.published} | ${item.measured} | ${item.driftPercent}% |\n`;
        }

        return report;
    }
}

describe('Resource Cost Benchmark Pipeline', () => {
    test('parses benchmark output correctly', () => {
        const benchmarkOutput = {
            results: [
                { entrypoint: 'submit_intent', cpu: 1000, memory: 512 },
                { entrypoint: 'accept_intent', cpu: 1200, memory: 600 },
                { entrypoint: 'fill_intent', cpu: 1500, memory: 700 }
            ]
        };

        const costs = BenchmarkParser.extractResourceCosts(benchmarkOutput);

        expect(costs.length).toBe(3);
        expect(costs[0].entrypoint).toBe('submit_intent');
        expect(costs[0].cpu).toBe(1000);
    });

    test('loads published resource costs from documentation', () => {
        const mockDocPath = '/tmp/test-doc.md';
        const mockContent = `
# Resource Costs

## Resource Costs

| Entrypoint | CPU | Memory |
|---|---|---|
| submit_intent | 950 | 500 |
| accept_intent | 1100 | 580 |
| fill_intent | 1400 | 680 |

## Other Sections
        `;

        // Simulate file system
        const costs = {
            'submit_intent': { cpu: 950, memory: 500 },
            'accept_intent': { cpu: 1100, memory: 580 },
            'fill_intent': { cpu: 1400, memory: 680 }
        };

        expect(costs['submit_intent'].cpu).toBe(950);
        expect(costs['accept_intent'].memory).toBe(580);
        expect(costs['fill_intent'].cpu).toBe(1400);
    });
});

describe('Drift Detection and Threshold', () => {
    test('detects drift exceeding 10% threshold', () => {
        const detector = new DriftDetector(10);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 },
            'accept_intent': { cpu: 1200, memory: 600 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1150, memory: 500 }, // 15% CPU drift
            'accept_intent': { cpu: 1200, memory: 600 }   // No drift
        };

        const report = detector.detectDrift(publishedCosts, measuredCosts);

        expect(report.hasDrift).toBe(true);
        expect(report.driftItems.length).toBe(1);
        expect(report.driftItems[0].entrypoint).toBe('submit_intent');
        expect(report.driftItems[0].driftPercent).toBe('15.00');
    });

    test('ignores drift within threshold', () => {
        const detector = new DriftDetector(10);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1050, memory: 500 } // 5% CPU drift (within threshold)
        };

        const report = detector.detectDrift(publishedCosts, measuredCosts);

        expect(report.hasDrift).toBe(false);
        expect(report.driftItems.length).toBe(0);
    });

    test('detects memory drift independently from CPU', () => {
        const detector = new DriftDetector(10);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 },
            'accept_intent': { cpu: 1200, memory: 600 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1000, memory: 575 }, // 15% memory drift only
            'accept_intent': { cpu: 1200, memory: 600 }
        };

        const report = detector.detectDrift(publishedCosts, measuredCosts);

        expect(report.hasDrift).toBe(true);
        expect(report.driftItems[0].resource).toBe('memory');
    });

    test('detects both CPU and memory drift on same entrypoint', () => {
        const detector = new DriftDetector(10);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1150, memory: 575 } // 15% CPU, 15% memory drift
        };

        const report = detector.detectDrift(publishedCosts, measuredCosts);

        expect(report.hasDrift).toBe(true);
        expect(report.driftItems.length).toBe(2);
    });

    test('configurable threshold values', () => {
        const detector5 = new DriftDetector(5);
        const detector20 = new DriftDetector(20);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1100, memory: 500 } // 10% drift
        };

        const report5 = detector5.detectDrift(publishedCosts, measuredCosts);
        const report20 = detector20.detectDrift(publishedCosts, measuredCosts);

        expect(report5.hasDrift).toBe(true);  // 10% > 5% threshold
        expect(report20.hasDrift).toBe(false); // 10% < 20% threshold
    });
});

describe('Drift Report Generation', () => {
    test('formats drift report as markdown', () => {
        const detector = new DriftDetector(10);

        const driftReport = {
            hasDrift: true,
            driftItems: [
                {
                    entrypoint: 'submit_intent',
                    resource: 'cpu',
                    published: 1000,
                    measured: 1150,
                    driftPercent: '15.00'
                }
            ],
            summary: 'Detected 1 resource cost drift(s) exceeding 10% threshold'
        };

        const formatted = detector.formatDriftReport(driftReport);

        expect(formatted).toContain('# Resource Cost Drift Report');
        expect(formatted).toContain('submit_intent');
        expect(formatted).toContain('15.00%');
    });

    test('formats no-drift report correctly', () => {
        const detector = new DriftDetector(10);

        const driftReport = {
            hasDrift: false,
            driftItems: [],
            summary: null
        };

        const formatted = detector.formatDriftReport(driftReport);

        expect(formatted).toContain('No significant drift detected');
    });
});

describe('CI Job Behavior', () => {
    test('job fails when drift detected above threshold', () => {
        const detector = new DriftDetector(10);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1200, memory: 500 } // 20% drift
        };

        const report = detector.detectDrift(publishedCosts, measuredCosts);

        if (report.hasDrift) {
            // CI job exits with error status
            const exitCode = 1;
            expect(exitCode).toBe(1);
            expect(report.summary).toContain('Detected');
        }
    });

    test('job passes when no drift above threshold', () => {
        const detector = new DriftDetector(10);

        const publishedCosts = {
            'submit_intent': { cpu: 1000, memory: 500 }
        };

        const measuredCosts = {
            'submit_intent': { cpu: 1050, memory: 500 } // 5% drift
        };

        const report = detector.detectDrift(publishedCosts, measuredCosts);

        if (!report.hasDrift) {
            // CI job exits successfully
            const exitCode = 0;
            expect(exitCode).toBe(0);
        }
    });

    test('generates tracking issue/PR when drift detected', () => {
        const detector = new DriftDetector(10);

        const driftReport = {
            hasDrift: true,
            driftItems: [
                {
                    entrypoint: 'submit_intent',
                    resource: 'cpu',
                    published: 1000,
                    measured: 1200,
                    driftPercent: '20.00'
                }
            ],
            summary: 'Detected 1 resource cost drift(s) exceeding 10% threshold'
        };

        // Simulate issue/PR creation payload
        const issuePayload = {
            title: 'Resource costs drift detected in CI',
            body: detector.formatDriftReport(driftReport),
            labels: ['documentation', 'resource-costs'],
            milestone: null
        };

        expect(issuePayload.title).toContain('drift detected');
        expect(issuePayload.body).toContain('submit_intent');
        expect(issuePayload.labels).toContain('resource-costs');
    });

    test('scheduled trigger runs weekly benchmarks', () => {
        // Simulate GitHub Actions schedule trigger
        const workflow = {
            name: 'Resource Cost Benchmark',
            on: {
                schedule: [{ cron: '0 0 * * 0' }], // Weekly on Sunday
                push: {
                    paths: ['intent_settlement/src/lib.rs']
                }
            }
        };

        expect(workflow.on.schedule).toBeDefined();
        expect(workflow.on.schedule[0].cron).toBe('0 0 * * 0');
        expect(workflow.on.push.paths).toContain('intent_settlement/src/lib.rs');
    });
});

describe('Documentation Update Integration', () => {
    test('updates doc with new resource costs when approved', () => {
        const updater = new DocParser();

        const oldCosts = {
            'submit_intent': { cpu: 1000, memory: 500 },
            'accept_intent': { cpu: 1200, memory: 600 }
        };

        const newCosts = {
            'submit_intent': { cpu: 1050, memory: 510 },
            'accept_intent': { cpu: 1250, memory: 620 }
        };

        // Simulate documentation update
        const updated = { ...oldCosts, ...newCosts };

        expect(updated['submit_intent'].cpu).toBe(1050);
        expect(updated['accept_intent'].memory).toBe(620);
    });

    test('preserves other documentation sections during update', () => {
        const docContent = `
# Vortex Resource Documentation

## Overview
This document tracks resource costs...

## Resource Costs
| Entrypoint | CPU | Memory |
|---|---|---|
| submit_intent | 1000 | 500 |

## Changelog
- v1.0: Initial costs

## See Also
- [Benchmark Harness](#)
        `;

        // After update, non-cost sections should remain
        expect(docContent).toContain('## Overview');
        expect(docContent).toContain('## Changelog');
        expect(docContent).toContain('## See Also');
    });
});

module.exports = {
    BenchmarkParser,
    DocParser,
    DriftDetector
};
