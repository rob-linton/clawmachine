import 'dart:typed_data';
import 'package:dio/dio.dart';
import '../models/job.dart';
import '../models/skill.dart';
import '../models/cron_schedule.dart';
import '../models/workspace.dart';
import '../models/tool.dart';
import '../models/credential.dart';

class ApiClient {
  final Dio _dio;
  final String _baseUrl;

  ApiClient(String baseUrl)
      : _baseUrl = baseUrl,
        _dio = Dio(BaseOptions(
          baseUrl: '$baseUrl/api/v1',
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
          extra: {'withCredentials': true},
        ));

  /// Build a direct URL for file download (opened in browser, not via Dio)
  String fileDownloadUrl(String workspaceId, String path, {bool download = false}) {
    final param = download ? 'download=true' : 'raw=true';
    return '$_baseUrl/api/v1/workspaces/$workspaceId/files/$path?$param';
  }

  /// Build a URL for workspace ZIP download
  String workspaceDownloadUrl(String workspaceId, {String? subpath}) {
    final query = subpath != null ? '?path=$subpath' : '';
    return '$_baseUrl/api/v1/workspaces/$workspaceId/download$query';
  }

  // Auth
  Future<Map<String, dynamic>> login(String username, String password) async {
    final resp = await _dio.post('/auth/login', data: {
      'username': username,
      'password': password,
    });
    return Map<String, dynamic>.from(resp.data);
  }

  Future<void> logout() async {
    await _dio.post('/auth/logout');
  }

  Future<Map<String, dynamic>> checkAuth() async {
    final resp = await _dio.get('/auth/me');
    return Map<String, dynamic>.from(resp.data);
  }

  // Jobs
  Future<Map<String, dynamic>> submitJob({
    required String prompt,
    List<String> skillIds = const [],
    List<String> skillTags = const [],
    List<String> toolIds = const [],
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
      if (toolIds.isNotEmpty) 'tool_ids': toolIds,
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

  // Tools
  Future<List<Tool>> listTools() async {
    final resp = await _dio.get('/tools');
    final items = resp.data['items'] as List? ?? [];
    return items.map((t) => Tool.fromJson(t)).toList();
  }

  Future<Tool> getTool(String id) async {
    final resp = await _dio.get('/tools/$id');
    return Tool.fromJson(resp.data);
  }

  Future<void> createTool(Tool tool) async {
    await _dio.post('/tools', data: tool.toJson());
  }

  Future<void> updateTool(String id, Tool tool) async {
    await _dio.put('/tools/$id', data: tool.toJson());
  }

  Future<void> deleteTool(String id) async {
    await _dio.delete('/tools/$id');
  }

  // Credentials
  Future<List<Credential>> listCredentials() async {
    final resp = await _dio.get('/credentials');
    final items = resp.data['items'] as List? ?? [];
    return items.map((c) => Credential.fromJson(c)).toList();
  }

  Future<void> createCredential({
    required String id,
    required String name,
    String description = '',
    required Map<String, String> values,
  }) async {
    await _dio.post('/credentials', data: {
      'id': id,
      'name': name,
      'description': description,
      'values': values,
    });
  }

  Future<void> updateCredential({
    required String id,
    required String name,
    String description = '',
    required Map<String, String> values,
  }) async {
    await _dio.put('/credentials/$id', data: {
      'id': id,
      'name': name,
      'description': description,
      'values': values,
    });
  }

  Future<void> deleteCredential(String id) async {
    await _dio.delete('/credentials/$id');
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

  Future<void> deleteWorkspaceFile(String id, String path) async {
    await _dio.delete('/workspaces/$id/files/$path');
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

  // Install from URL
  Future<Skill> installSkillFromUrl({required String url, String? path}) async {
    final resp = await _dio.post('/skills/install-from-url', data: {
      'url': url,
      if (path != null) 'path': path,
    });
    return Skill.fromJson(resp.data);
  }

  Future<Tool> installToolFromUrl({required String url, String? path}) async {
    final resp = await _dio.post('/tools/install-from-url', data: {
      'url': url,
      if (path != null) 'path': path,
    });
    return Tool.fromJson(resp.data);
  }

  Future<Skill> updateSkillFromSource(String id) async {
    final resp = await _dio.post('/skills/$id/update-from-source');
    return Skill.fromJson(resp.data);
  }

  Future<Tool> updateToolFromSource(String id) async {
    final resp = await _dio.post('/tools/$id/update-from-source');
    return Tool.fromJson(resp.data);
  }

  // Catalog
  Future<Map<String, dynamic>> fetchCatalog() async {
    final configResp = await _dio.get('/config/catalog_url');
    final rawValue = configResp.data['value'] ?? configResp.data['catalog_url'] ?? '';
    final catalogUrl = rawValue is String ? rawValue : rawValue.toString();
    if (catalogUrl.isEmpty) {
      return {'skills': [], 'tools': []};
    }
    // Fetch the catalog directly (it's a public JSON file)
    final resp = await Dio().get(catalogUrl);
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

  /// Build a URL for skill ZIP download
  String skillDownloadUrl(String id) => '$_baseUrl/api/v1/skills/$id/download';

  /// Build a URL for tool ZIP download
  String toolDownloadUrl(String id) => '$_baseUrl/api/v1/tools/$id/download';

  Future<Tool> uploadToolZip(Uint8List zipBytes, {
    required String id, required String name,
    String description = '', List<String> tags = const [],
  }) async {
    final formData = FormData.fromMap({
      'file': MultipartFile.fromBytes(zipBytes, filename: 'tool.zip'),
      'id': id, 'name': name,
      'description': description, 'tags': tags.join(','),
    });
    final resp = await _dio.post('/tools/upload', data: formData);
    return Tool.fromJson(resp.data);
  }

  // System Config
  Future<Map<String, dynamic>> getConfig() async {
    final resp = await _dio.get('/config');
    return Map<String, dynamic>.from(resp.data);
  }

  Future<void> updateConfig(Map<String, String> values) async {
    await _dio.put('/config', data: values);
  }

  Future<void> setConfigValue(String key, String value) async {
    await _dio.put('/config/$key', data: {'value': value});
  }

  // Docker Management
  Future<Map<String, dynamic>> getDockerStatus() async {
    final resp = await _dio.get('/docker/status');
    return Map<String, dynamic>.from(resp.data);
  }

  Future<List<dynamic>> getDockerImages() async {
    final resp = await _dio.get('/docker/images');
    return resp.data['images'] as List? ?? [];
  }

  Future<Map<String, dynamic>> pullDockerImage({String? image}) async {
    final resp = await _dio.post('/docker/images/pull', data: {
      if (image != null) 'image': image,
    });
    return Map<String, dynamic>.from(resp.data);
  }

  Future<Map<String, dynamic>> buildDockerImage({String? tag}) async {
    final resp = await _dio.post('/docker/images/build', data: {
      if (tag != null) 'tag': tag,
    });
    return Map<String, dynamic>.from(resp.data);
  }

  // Workspace sync & promote
  Future<void> syncWorkspace(String id) async {
    await _dio.post('/workspaces/$id/sync');
  }

  Future<void> promoteSnapshot(String id, String ref) async {
    await _dio.post('/workspaces/$id/promote', queryParameters: {'ref': ref});
  }

  Future<Workspace> forkWorkspace(String id, Map<String, dynamic> data) async {
    final resp = await _dio.post('/workspaces/$id/fork', data: data);
    return Workspace.fromJson(resp.data);
  }

  Future<Map<String, dynamic>> listWorkspaceEvents(String id, {int limit = 50, int offset = 0}) async {
    final resp = await _dio.get('/workspaces/$id/events', queryParameters: {
      'limit': limit,
      'offset': offset,
    });
    return Map<String, dynamic>.from(resp.data);
  }

  Future<List<dynamic>> listWorkspaceBranches(String id) async {
    final resp = await _dio.get('/workspaces/$id/branches');
    return resp.data['branches'] as List? ?? [];
  }

  // Extended status
  Future<Map<String, dynamic>> getFullStatus() async {
    final resp = await _dio.get('/status');
    return Map<String, dynamic>.from(resp.data);
  }

  // OAuth
  Future<Map<String, dynamic>> getOAuthStatus() async {
    final resp = await _dio.get('/auth/oauth-status');
    return Map<String, dynamic>.from(resp.data);
  }

  // Catalog sync
  Future<Map<String, dynamic>> syncCatalog() async {
    final resp = await _dio.post('/catalog/sync');
    return Map<String, dynamic>.from(resp.data);
  }

}
