import 'dart:typed_data';
import 'package:dio/dio.dart';
import '../models/job.dart';
import '../models/skill.dart';
import '../models/cron_schedule.dart';
import '../models/workspace.dart';

class ApiClient {
  final Dio _dio;

  ApiClient(String baseUrl)
      : _dio = Dio(BaseOptions(
          baseUrl: '$baseUrl/api/v1',
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
        ));

  // Jobs
  Future<Map<String, dynamic>> submitJob({
    required String prompt,
    List<String> skillIds = const [],
    List<String> skillTags = const [],
    String? model,
    double? maxBudget,
    int? priority,
    List<String> tags = const [],
    String? workingDir,
    int? timeoutSecs,
    Map<String, dynamic>? outputDest,
    List<String>? allowedTools,
    String? workspaceId,
  }) async {
    final resp = await _dio.post('/jobs', data: {
      'prompt': prompt,
      if (skillIds.isNotEmpty) 'skill_ids': skillIds,
      if (skillTags.isNotEmpty) 'skill_tags': skillTags,
      if (model != null) 'model': model,
      if (maxBudget != null) 'max_budget_usd': maxBudget,
      if (priority != null) 'priority': priority,
      if (tags.isNotEmpty) 'tags': tags,
      if (workingDir != null) 'working_dir': workingDir,
      if (timeoutSecs != null) 'timeout_secs': timeoutSecs,
      if (outputDest != null) 'output_dest': outputDest,
      if (allowedTools != null && allowedTools.isNotEmpty)
        'allowed_tools': allowedTools,
      if (workspaceId != null) 'workspace_id': workspaceId,
    });
    return resp.data;
  }

  Future<List<Job>> listJobs({String? status, int limit = 20}) async {
    final params = <String, dynamic>{'limit': limit};
    if (status != null) params['status'] = status;
    final resp = await _dio.get('/jobs', queryParameters: params);
    final items = resp.data['items'] as List? ?? [];
    return items.map((j) => Job.fromJson(j)).toList();
  }

  Future<Job> getJob(String id) async {
    final resp = await _dio.get('/jobs/$id');
    return Job.fromJson(resp.data);
  }

  Future<JobResult> getResult(String id) async {
    final resp = await _dio.get('/jobs/$id/result');
    return JobResult.fromJson(resp.data);
  }

  Future<List<String>> getLogs(String id) async {
    final resp = await _dio.get('/jobs/$id/logs');
    return List<String>.from(resp.data['lines'] ?? []);
  }

  Future<void> cancelJob(String id) async {
    await _dio.post('/jobs/$id/cancel');
  }

  Future<void> deleteJob(String id) async {
    await _dio.delete('/jobs/$id');
  }

  Future<QueueStatus> getStatus() async {
    final resp = await _dio.get('/status');
    return QueueStatus.fromJson(resp.data['queue']);
  }

  // Skills
  Future<List<Skill>> listSkills() async {
    final resp = await _dio.get('/skills');
    final items = resp.data['items'] as List? ?? [];
    return items.map((s) => Skill.fromJson(s)).toList();
  }

  Future<Skill> getSkill(String id) async {
    final resp = await _dio.get('/skills/$id');
    return Skill.fromJson(resp.data);
  }

  Future<void> createSkill(Skill skill) async {
    await _dio.post('/skills', data: skill.toJson());
  }

  Future<void> updateSkill(String id, Skill skill) async {
    await _dio.put('/skills/$id', data: skill.toJson());
  }

  Future<void> deleteSkill(String id) async {
    await _dio.delete('/skills/$id');
  }

  // Crons
  Future<List<CronSchedule>> listCrons() async {
    final resp = await _dio.get('/crons');
    final items = resp.data['items'] as List? ?? [];
    return items.map((c) => CronSchedule.fromJson(c)).toList();
  }

  Future<CronSchedule> getCron(String id) async {
    final resp = await _dio.get('/crons/$id');
    return CronSchedule.fromJson(resp.data);
  }

  Future<CronSchedule> createCron(Map<String, dynamic> data) async {
    final resp = await _dio.post('/crons', data: data);
    return CronSchedule.fromJson(resp.data);
  }

  Future<CronSchedule> updateCron(String id, Map<String, dynamic> data) async {
    final resp = await _dio.put('/crons/$id', data: data);
    return CronSchedule.fromJson(resp.data);
  }

  Future<void> deleteCron(String id) async {
    await _dio.delete('/crons/$id');
  }

  Future<Map<String, dynamic>> triggerCron(String id) async {
    final resp = await _dio.post('/crons/$id/trigger');
    return resp.data;
  }

  // Workspaces
  Future<List<Workspace>> listWorkspaces() async {
    final resp = await _dio.get('/workspaces');
    final items = resp.data['items'] as List? ?? [];
    return items.map((w) => Workspace.fromJson(w)).toList();
  }

  Future<Workspace> getWorkspace(String id) async {
    final resp = await _dio.get('/workspaces/$id');
    return Workspace.fromJson(resp.data);
  }

  Future<Workspace> createWorkspace(Map<String, dynamic> data) async {
    final resp = await _dio.post('/workspaces', data: data);
    return Workspace.fromJson(resp.data);
  }

  Future<Workspace> updateWorkspace(String id, Map<String, dynamic> data) async {
    final resp = await _dio.put('/workspaces/$id', data: data);
    return Workspace.fromJson(resp.data);
  }

  Future<void> deleteWorkspace(String id, {bool deleteFiles = false}) async {
    await _dio.delete('/workspaces/$id', queryParameters: {
      if (deleteFiles) 'delete_files': 'true',
    });
  }

  Future<List<dynamic>> listWorkspaceFiles(String id) async {
    final resp = await _dio.get('/workspaces/$id/files');
    return resp.data['files'] as List? ?? [];
  }

  Future<String> getWorkspaceFile(String id, String path) async {
    final resp = await _dio.get('/workspaces/$id/files/$path');
    return resp.data['content'] ?? '';
  }

  Future<void> putWorkspaceFile(String id, String path, String content) async {
    await _dio.put('/workspaces/$id/files/$path', data: {'content': content});
  }

  Future<Map<String, dynamic>> uploadWorkspaceZip(String workspaceId, Uint8List zipBytes, {String? prefix}) async {
    final formData = FormData.fromMap({
      'file': MultipartFile.fromBytes(zipBytes, filename: 'upload.zip'),
      if (prefix != null && prefix.isNotEmpty) 'path': prefix,
    });
    final resp = await _dio.post('/workspaces/$workspaceId/upload', data: formData);
    return Map<String, dynamic>.from(resp.data);
  }

  // Job Templates
  Future<List<dynamic>> listJobTemplates() async {
    final resp = await _dio.get('/job-templates');
    return resp.data['items'] as List? ?? [];
  }

  Future<Map<String, dynamic>> createJobTemplate(Map<String, dynamic> data) async {
    final resp = await _dio.post('/job-templates', data: data);
    return Map<String, dynamic>.from(resp.data);
  }

  Future<Map<String, dynamic>> updateJobTemplate(String id, Map<String, dynamic> data) async {
    final resp = await _dio.put('/job-templates/$id', data: data);
    return Map<String, dynamic>.from(resp.data);
  }

  Future<void> deleteJobTemplate(String id) async {
    await _dio.delete('/job-templates/$id');
  }

  Future<Map<String, dynamic>> runJobTemplate(String id) async {
    final resp = await _dio.post('/job-templates/$id/run');
    return Map<String, dynamic>.from(resp.data);
  }

  // Workspace git history
  Future<List<dynamic>> getWorkspaceHistory(String id) async {
    final resp = await _dio.get('/workspaces/$id/history');
    return resp.data['commits'] as List? ?? [];
  }

  Future<void> revertWorkspaceCommit(String id, String hash) async {
    await _dio.post('/workspaces/$id/revert/$hash');
  }

  // Pipelines
  Future<List<dynamic>> listPipelines() async {
    final resp = await _dio.get('/pipelines');
    return resp.data['items'] as List? ?? [];
  }

  Future<Map<String, dynamic>> createPipeline(Map<String, dynamic> data) async {
    final resp = await _dio.post('/pipelines', data: data);
    return Map<String, dynamic>.from(resp.data);
  }

  Future<void> deletePipeline(String id) async {
    await _dio.delete('/pipelines/$id');
  }

  Future<Map<String, dynamic>> runPipeline(String id) async {
    final resp = await _dio.post('/pipelines/$id/run');
    return Map<String, dynamic>.from(resp.data);
  }

  Future<List<dynamic>> listPipelineRuns() async {
    final resp = await _dio.get('/pipeline-runs');
    return resp.data['items'] as List? ?? [];
  }

  Future<Map<String, dynamic>> getPipeline(String id) async {
    final resp = await _dio.get('/pipelines/$id');
    return Map<String, dynamic>.from(resp.data);
  }

  Future<Map<String, dynamic>> getPipelineRun(String id) async {
    final resp = await _dio.get('/pipeline-runs/$id');
    return Map<String, dynamic>.from(resp.data);
  }

  Future<Skill> uploadSkillZip(Uint8List zipBytes, {
    required String id, required String name,
    String description = '', List<String> tags = const [],
  }) async {
    final formData = FormData.fromMap({
      'file': MultipartFile.fromBytes(zipBytes, filename: 'skill.zip'),
      'id': id, 'name': name,
      'description': description, 'tags': tags.join(','),
    });
    final resp = await _dio.post('/skills/upload', data: formData);
    return Skill.fromJson(resp.data);
  }
}
