package helloworld;

import java.util.concurrent.Executor;

import com.alibaba.nacos.api.NacosFactory;
import com.alibaba.nacos.api.config.ConfigService;
import com.alibaba.nacos.api.config.listener.Listener;
import com.alibaba.nacos.api.exception.NacosException;
import com.amazonaws.services.lambda.runtime.Context;
import com.amazonaws.services.lambda.runtime.RequestHandler;
import com.amazonaws.services.lambda.runtime.events.APIGatewayProxyRequestEvent;
import com.amazonaws.services.lambda.runtime.events.APIGatewayProxyResponseEvent;

public class App implements RequestHandler<APIGatewayProxyRequestEvent, APIGatewayProxyResponseEvent> {
  ConfigService configService;
  String config;

  public App() throws NacosException {
    var addr = System.getenv("NACOS_SERVER_ADDRESS");
    if (addr == null)
      addr = "localhost:8848";
    var dataId = System.getenv("NACOS_DATA_ID");
    if (dataId == null)
      dataId = "test";
    var group = System.getenv("NACOS_GROUP");
    if (group == null)
      group = "DEFAULT_GROUP";

    configService = NacosFactory.createConfigService(addr);

    // get the initial config
    config = configService.getConfig(dataId, group, 5000);
    if (config == null)
      throw new NacosException(NacosException.NOT_FOUND, "Failed to get config");

    // add a listener
    configService.addListener(dataId, group, new Listener() {
      @Override
      public void receiveConfigInfo(String configInfo) {
        config = configInfo;
      }

      @Override
      public Executor getExecutor() {
        return null;
      }
    });
  }

  public APIGatewayProxyResponseEvent handleRequest(final APIGatewayProxyRequestEvent input, final Context context) {
    return new APIGatewayProxyResponseEvent()
        .withStatusCode(200)
        .withBody(config);
  }
}
